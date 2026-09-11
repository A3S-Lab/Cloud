use crate::modules::edge::domain::EdgeMcpServiceProfileProjectionBinding;
use crate::modules::edge::domain::EdgeMcpWorkloadRevisionProjectionBinding;
use crate::modules::edge::domain::{DomainClaim, GatewayScope, McpRoutePolicy};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMcpRouteProjectionInput {
    pub policy: McpRoutePolicy,
    pub domain_claim: DomainClaim,
    pub profile_binding: EdgeMcpServiceProfileProjectionBinding,
    pub revision_binding: EdgeMcpWorkloadRevisionProjectionBinding,
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
