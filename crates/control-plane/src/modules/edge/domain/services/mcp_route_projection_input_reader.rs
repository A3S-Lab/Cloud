use crate::modules::assets::domain::McpServiceProfileBinding;
use crate::modules::edge::domain::{GatewayScope, McpRoutePolicy};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::entities::WorkloadRevision;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMcpRouteProjectionInput {
    pub policy: McpRoutePolicy,
    pub profile_binding: McpServiceProfileBinding,
    pub revision: WorkloadRevision,
    /// Optimistic version of the Workload whose active revision was read.
    pub workload_aggregate_version: u64,
}

#[async_trait]
pub trait IMcpRouteProjectionInputReader: Send + Sync {
    /// Materializes every active desired MCP route for one exact Gateway
    /// scope. Returning a partial set is forbidden.
    async fn list_active_projection_inputs(
        &self,
        scope: &GatewayScope,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<ResolvedMcpRouteProjectionInput>, RepositoryError>;
}
