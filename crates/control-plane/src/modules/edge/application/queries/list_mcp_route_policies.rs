use crate::modules::edge::application::McpRoutePolicyApplicationService;
use crate::modules::edge::application::resource_access::EdgeAccess;
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListMcpRoutePolicies {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub access: EdgeAccess,
}

impl Query for ListMcpRoutePolicies {
    type Output = ApplicationResult<Vec<McpRoutePolicy>>;
}

pub struct ListMcpRoutePoliciesHandler {
    service: Arc<McpRoutePolicyApplicationService>,
}

impl ListMcpRoutePoliciesHandler {
    pub fn new(service: Arc<McpRoutePolicyApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListMcpRoutePolicies> for ListMcpRoutePoliciesHandler {
    fn execute(
        &self,
        query: ListMcpRoutePolicies,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<McpRoutePolicy>>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "MCP route policies not found".into(),
                )));
            }
            Ok(service
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::application::mcp_service_profile_access::{
        EdgeMcpServiceProfileScope, IEdgeMcpServiceProfileAccess,
    };
    use crate::modules::edge::application::resource_access::EdgeAccessScope;
    use crate::modules::edge::domain::repositories::{
        IMcpRoutePolicyRepository, McpRoutePolicyWrite, MutateMcpRoutePolicyWrite,
    };
    use crate::modules::edge::domain::{
        EdgeMcpServiceProfileAdmission, EdgeMcpServiceProfileProjectionBinding,
    };
    use crate::modules::shared_kernel::domain::{GatewayScopeId, RepositoryError, RouteId};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};

    struct StubPolicies;

    #[async_trait]
    impl IMcpRoutePolicyRepository for StubPolicies {
        async fn mutate_mcp_route_policy(
            &self,
            _write: MutateMcpRoutePolicyWrite,
        ) -> Result<McpRoutePolicyWrite, RepositoryError> {
            panic!("visibility gate must fail closed before mutating policies");
        }

        async fn find_mcp_route_policy(
            &self,
            _organization_id: OrganizationId,
            _route_id: RouteId,
        ) -> Result<Option<McpRoutePolicy>, RepositoryError> {
            panic!("visibility gate must fail closed before finding policies");
        }

        async fn list_mcp_route_policies(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
        ) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
            panic!("visibility gate must fail closed before listing policies");
        }

        async fn list_active_mcp_route_policies_for_gateway(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            _gateway_scope_id: GatewayScopeId,
            _active_at: DateTime<Utc>,
        ) -> Result<Vec<McpRoutePolicy>, RepositoryError> {
            panic!("visibility gate must fail closed before listing gateway policies");
        }
    }

    struct StubProfiles;

    #[async_trait]
    impl IEdgeMcpServiceProfileAccess for StubProfiles {
        async fn find_bound_profile(
            &self,
            _scope: EdgeMcpServiceProfileScope,
        ) -> Result<Option<EdgeMcpServiceProfileAdmission>, RepositoryError> {
            panic!("visibility gate must fail closed before profile admission");
        }

        async fn find_projection_binding(
            &self,
            _scope: EdgeMcpServiceProfileScope,
        ) -> Result<Option<EdgeMcpServiceProfileProjectionBinding>, RepositoryError> {
            panic!("visibility gate must fail closed before projection binding");
        }
    }

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let service = Arc::new(McpRoutePolicyApplicationService::new(
            Arc::new(StubPolicies),
            Arc::new(StubProfiles),
        ));
        let handler = ListMcpRoutePoliciesHandler::new(service);
        let result = handler
            .execute(
                ListMcpRoutePolicies {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: EdgeAccess::restricted([EdgeAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute list query");

        assert_eq!(
            result,
            Err(ApplicationError::NotFound(
                "MCP route policies not found".into()
            ))
        );
    }
}
