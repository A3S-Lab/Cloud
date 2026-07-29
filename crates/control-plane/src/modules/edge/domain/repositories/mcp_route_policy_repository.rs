use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use async_trait::async_trait;

#[async_trait]
pub trait IMcpRoutePolicyRepository: Send + Sync {
    async fn create_mcp_route_policy(
        &self,
        policy: McpRoutePolicy,
    ) -> Result<McpRoutePolicy, RepositoryError>;

    async fn update_mcp_route_policy(
        &self,
        policy: McpRoutePolicy,
        expected_policy_revision: u64,
    ) -> Result<McpRoutePolicy, RepositoryError>;

    async fn find_mcp_route_policy(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Option<McpRoutePolicy>, RepositoryError>;

    async fn list_mcp_route_policies(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError>;
}
