use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AssetId, OrganizationId, WorkloadId};
use crate::modules::workloads::application::UpdateWorkloadDeploymentResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UnbindSkillWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub skill_asset_id: AssetId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for UnbindSkillWorkloadDeployment {
    type Output = ApplicationResult<UpdateWorkloadDeploymentResult>;
}
