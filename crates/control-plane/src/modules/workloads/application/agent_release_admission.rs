use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use crate::modules::workloads::domain::entities::AgentReleaseAdmission;
use async_trait::async_trait;

/// Workloads-owned request for admitting one exact immutable Agent release.
///
/// Assets and Artifacts remain authoritative for release and artifact state.
/// Workloads receives only the admission needed by its own revision binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadAgentReleaseAdmissionRequest {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
}

#[async_trait]
pub trait IWorkloadAgentReleaseAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: WorkloadAgentReleaseAdmissionRequest,
    ) -> ApplicationResult<AgentReleaseAdmission>;
}
