use super::{ChangeNodeState, ChangeNodeStateResult};
use crate::modules::fleet::application::certificate;
use crate::modules::fleet::domain::events::NodeStateChanged;
use crate::modules::fleet::domain::repositories::{INodeRepository, NodeStateChange};
use crate::modules::fleet::domain::services::ICertificateAuthority;
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct ChangeNodeStateHandler {
    nodes: Arc<dyn INodeRepository>,
    certificate_authority: Arc<dyn ICertificateAuthority>,
}

impl ChangeNodeStateHandler {
    pub fn new(
        nodes: Arc<dyn INodeRepository>,
        certificate_authority: Arc<dyn ICertificateAuthority>,
    ) -> Self {
        Self {
            nodes,
            certificate_authority,
        }
    }
}

impl CommandHandler<ChangeNodeState> for ChangeNodeStateHandler {
    fn execute(
        &self,
        command: ChangeNodeState,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ChangeNodeStateResult>>>
    {
        let nodes = Arc::clone(&self.nodes);
        let certificate_authority = Arc::clone(&self.certificate_authority);
        Box::pin(async move {
            if command.state == NodeState::Pending || command.expected_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "node state and expected version are invalid".into(),
                )));
            }
            let current = match nodes.find(command.organization_id, command.node_id).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "nodeId": command.node_id,
                "state": command.state.as_str(),
                "expectedVersion": command.expected_version,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/nodes/{}/state",
                    command.organization_id, command.node_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };

            if command.state == NodeState::Revoked && current.state != NodeState::Revoked {
                let active_certificate = match nodes
                    .find_active_certificate(command.organization_id, command.node_id)
                    .await
                {
                    Ok(value) => value,
                    Err(error) => return Ok(Err(error.into())),
                };
                if let Err(error) = certificate_authority.revoke(&active_certificate).await {
                    return Ok(Err(certificate::application_error(error)));
                }
            }

            let mut projected = current.clone();
            let transition = match command.state {
                NodeState::Ready => projected.mark_ready(),
                NodeState::Draining => projected.drain(),
                NodeState::Revoked => {
                    projected.revoke();
                    Ok(())
                }
                NodeState::Pending => Err("node cannot transition back to pending".into()),
            };
            if let Err(error) = transition {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = NodeStateChanged::envelope(
                &projected,
                command.state,
                command.requested_at,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            match nodes
                .set_state(NodeStateChange {
                    organization_id: command.organization_id,
                    node_id: command.node_id,
                    state: command.state,
                    expected_version: command.expected_version,
                    changed_at: command.requested_at,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(ChangeNodeStateResult {
                    node: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
