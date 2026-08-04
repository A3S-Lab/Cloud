use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, EnvironmentId, OrganizationId, ProjectId,
};
use crate::modules::workloads::application::{
    CreateWorkloadDeploymentResult, SourceWorkloadTemplate,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateAgentWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub name: String,
    pub template: SourceWorkloadTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateAgentWorkloadDeployment {
    type Output = ApplicationResult<CreateWorkloadDeploymentResult>;
}
