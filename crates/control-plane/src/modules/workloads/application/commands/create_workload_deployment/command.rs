use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::workloads::domain::entities::RequestedServiceTemplate;
use crate::modules::workloads::domain::repositories::DeploymentBundle;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateWorkloadDeployment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: String,
    pub template: RequestedServiceTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateWorkloadDeployment {
    type Output = ApplicationResult<CreateWorkloadDeploymentResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateWorkloadDeploymentResult {
    pub bundle: DeploymentBundle,
}
