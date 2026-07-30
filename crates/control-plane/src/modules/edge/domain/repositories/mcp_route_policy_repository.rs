use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

pub const MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY: usize = 1_000;

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

    /// Reads the complete active desired-route set for one exact logical
    /// Gateway scope. Implementations must fail rather than truncate when the
    /// fixed projection bound is exceeded.
    async fn list_active_mcp_route_policies_for_gateway(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        gateway_scope_id: GatewayScopeId,
        active_at: DateTime<Utc>,
    ) -> Result<Vec<McpRoutePolicy>, RepositoryError>;
}
