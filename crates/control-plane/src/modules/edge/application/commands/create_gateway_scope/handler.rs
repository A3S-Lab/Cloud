use super::{CreateGatewayScope, CreateGatewayScopeResult};
use crate::modules::edge::application::{
    EdgeEnvironmentScope, EdgeNodeScope, IEdgeEnvironmentAccess, IEdgeNodeAccess,
};
use crate::modules::edge::domain::events::GatewayScopeCreated;
use crate::modules::edge::domain::repositories::{CreateGatewayScopeWrite, IEdgeRepository};
use crate::modules::edge::domain::{GatewayRolloutPolicy, GatewayScope};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{GatewayScopeId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CreateGatewayScopeHandler {
    environments: Arc<dyn IEdgeEnvironmentAccess>,
    nodes: Arc<dyn IEdgeNodeAccess>,
    edge: Arc<dyn IEdgeRepository>,
}

impl CreateGatewayScopeHandler {
    pub fn new(
        environments: Arc<dyn IEdgeEnvironmentAccess>,
        nodes: Arc<dyn IEdgeNodeAccess>,
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
            let environment_scope = match EdgeEnvironmentScope::new(
                command.organization_id,
                command.project_id,
                command.environment_id,
            ) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match environments.environment_exists(environment_scope).await {
                Ok(true) => {}
                Ok(false) => {
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
                let node_scope = match EdgeNodeScope::new(command.organization_id, *node_id) {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
                match nodes.node_exists(node_scope).await {
                    Ok(true) => {}
                    Ok(false) => {
                        return Ok(Err(ApplicationError::NotFound("resource not found".into())))
                    }
                    Err(error) => return Ok(Err(error.into())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::GatewayRolloutPolicy;
    use crate::modules::edge::infrastructure::persistence::InMemoryEdgeRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError,
    };
    use a3s_boot::{CommandHandler, ModuleRef};
    use async_trait::async_trait;
    use chrono::Utc;
    use uuid::Uuid;

    struct MissingEnvironmentAccess;
    struct PresentEnvironmentAccess;
    struct MissingNodeAccess;
    struct PresentNodeAccess;

    #[async_trait]
    impl IEdgeEnvironmentAccess for MissingEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: EdgeEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl IEdgeEnvironmentAccess for PresentEnvironmentAccess {
        async fn environment_exists(
            &self,
            _scope: EdgeEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl IEdgeNodeAccess for MissingNodeAccess {
        async fn node_exists(&self, _scope: EdgeNodeScope) -> Result<bool, RepositoryError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl IEdgeNodeAccess for PresentNodeAccess {
        async fn node_exists(&self, _scope: EdgeNodeScope) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    fn command(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
    ) -> CreateGatewayScope {
        CreateGatewayScope {
            organization_id,
            project_id,
            environment_id,
            node_id,
            member_node_ids: vec![node_id],
            rollout_policy: GatewayRolloutPolicy::single_replica(),
            idempotency_key: "edge-gateway-scope-1".into(),
            request_id: Uuid::new_v4(),
            requested_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn missing_environment_fails_closed_as_not_found() {
        let handler = CreateGatewayScopeHandler::new(
            Arc::new(MissingEnvironmentAccess),
            Arc::new(PresentNodeAccess),
            Arc::new(InMemoryEdgeRepository::new()),
        );
        let error = handler
            .execute(
                command(
                    OrganizationId::new(),
                    ProjectId::new(),
                    EnvironmentId::new(),
                    NodeId::new(),
                ),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command bus")
            .expect_err("missing environment");
        assert!(matches!(error, ApplicationError::NotFound(_)));
    }

    #[tokio::test]
    async fn missing_member_node_fails_closed_as_not_found() {
        let handler = CreateGatewayScopeHandler::new(
            Arc::new(PresentEnvironmentAccess),
            Arc::new(MissingNodeAccess),
            Arc::new(InMemoryEdgeRepository::new()),
        );
        let error = handler
            .execute(
                command(
                    OrganizationId::new(),
                    ProjectId::new(),
                    EnvironmentId::new(),
                    NodeId::new(),
                ),
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("command bus")
            .expect_err("missing node");
        assert!(matches!(error, ApplicationError::NotFound(_)));
    }
}
