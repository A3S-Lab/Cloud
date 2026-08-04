use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, AssetReleaseId, OrganizationId, WorkloadId};
use crate::modules::workloads::application::UpdateWorkloadDeploymentResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BindSkillWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub skill_asset_id: AssetId,
    pub skill_asset_release_id: AssetReleaseId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for BindSkillWorkloadDeployment {
    type Output = ApplicationResult<UpdateWorkloadDeploymentResult>;
}
