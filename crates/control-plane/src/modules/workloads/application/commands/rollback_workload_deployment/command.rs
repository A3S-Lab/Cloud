use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId, WorkloadRevisionId};
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RollbackWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub source_revision_id: WorkloadRevisionId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for RollbackWorkloadDeployment {
    type Output = ApplicationResult<RollbackWorkloadDeploymentResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RollbackWorkloadDeploymentResult {
    pub bundle: DeploymentBundle,
    pub source_revision_id: WorkloadRevisionId,
}
