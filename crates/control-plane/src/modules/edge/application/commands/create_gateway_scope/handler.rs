use super::{CreateGatewayScope, CreateGatewayScopeResult};
use crate::modules::edge::domain::events::GatewayScopeCreated;
use crate::modules::edge::domain::repositories::{CreateGatewayScopeWrite, IEdgeRepository};
use crate::modules::edge::domain::{GatewayRolloutPolicy, GatewayScope};
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
            let scope = match GatewayScope::create_replicated(
                GatewayScopeId::new(),
                command.organization_id,
                command.project_id,
                command.environment_id,
                command.node_id,
                command.member_node_ids,
                command.rollout_policy,
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = if scope.member_node_ids.len() == 1
                && scope.rollout_policy == GatewayRolloutPolicy::single_replica()
            {
                serde_json::json!({
                    "organization_id": scope.organization_id,
                    "project_id": scope.project_id,
                    "environment_id": scope.environment_id,
                    "node_id": scope.node_id,
                })
            } else {
                serde_json::json!({
                    "organization_id": scope.organization_id,
                    "project_id": scope.project_id,
                    "environment_id": scope.environment_id,
                    "primary_node_id": scope.node_id,
                    "member_node_ids": scope.member_node_ids,
                    "rollout_policy": scope.rollout_policy,
                })
            };
            let canonical = serde_json::to_vec(&canonical)
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
            for node_id in &scope.member_node_ids {
                if let Err(error) = nodes.find(command.organization_id, *node_id).await {
                    return Ok(Err(error.into()));
                }
            }
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
