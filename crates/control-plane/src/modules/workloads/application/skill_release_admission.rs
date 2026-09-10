use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId};
use crate::modules::workloads::domain::entities::SkillReleaseAdmission;
use async_trait::async_trait;

/// Workloads-owned request for admitting one exact immutable Skill release.
///
/// Assets remains authoritative for Skill Asset and published bundle state.
/// Workloads receives only the admission needed by its own revision binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadSkillReleaseAdmissionRequest {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
}

#[async_trait]
pub trait IWorkloadSkillReleaseAdmissionPort: Send + Sync {
    async fn admit(
        &self,
        request: WorkloadSkillReleaseAdmissionRequest,
    ) -> ApplicationResult<SkillReleaseAdmission>;
}
