use super::{CreateGatewayScope, CreateGatewayScopeResult};
use crate::modules::edge::domain::events::GatewayScopeCreated;
use crate::modules::edge::domain::repositories::{CreateGatewayScopeWrite, IEdgeRepository};
use crate::modules::edge::domain::GatewayScope;
use crate::modules::fleet::domain::repositories::INodeRepository;
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{GatewayScopeId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateGatewayScopeHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    nodes: Arc<dyn INodeRepository>,
    edge: Arc<dyn IEdgeRepository>,
}

impl CreateGatewayScopeHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        nodes: Arc<dyn INodeRepository>,
        edge: Arc<dyn IEdgeRepository>,
    ) -> Self {
        Self {
            environments,
            nodes,
            edge,
        }
    }
}

impl CommandHandler<CreateGatewayScope> for CreateGatewayScopeHandler {
    fn execute(
        &self,
        command: CreateGatewayScope,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateGatewayScopeResult>>>
    {
        let environments = Arc::clone(&self.environments);
        let nodes = Arc::clone(&self.nodes);
        let edge = Arc::clone(&self.edge);
        Box::pin(async move {
            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if let Err(error) = nodes.find(command.organization_id, command.node_id).await {
                return Ok(Err(error.into()));
            }

            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "project_id": command.project_id,
                "environment_id": command.environment_id,
                "node_id": command.node_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/gateway-scopes",
                    command.organization_id, command.project_id, command.environment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let scope = GatewayScope::create(
                GatewayScopeId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.node_id,
                command.requested_at,
            );
            let event = GatewayScopeCreated::envelope(&scope, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match edge
                .create_gateway_scope(CreateGatewayScopeWrite {
                    scope,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateGatewayScopeResult {
                scope: write.value,
                replayed: write.replayed,
            }))
        })
    }
}
