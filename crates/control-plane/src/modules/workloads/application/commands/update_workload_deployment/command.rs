use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId};
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct UpdateWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub template: RequestedServiceTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for UpdateWorkloadDeployment {
    type Output = ApplicationResult<UpdateWorkloadDeploymentResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateWorkloadDeploymentResult {
    pub bundle: DeploymentBundle,
}
