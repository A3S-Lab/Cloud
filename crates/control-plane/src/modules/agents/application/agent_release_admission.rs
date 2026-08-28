use crate::modules::agents::domain::AgentReleaseBinding;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use async_trait::async_trait;

/// Agents-owned request for admitting one exact immutable Agent release.
///
/// Assets and Artifacts remain authoritative for release and artifact state.
/// Agents receives only the execution binding needed by its own aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentReleaseAdmissionRequest {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
}

#[async_trait]
pub trait IAgentReleaseAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: AgentReleaseAdmissionRequest,
    ) -> ApplicationResult<AgentReleaseBinding>;
}
