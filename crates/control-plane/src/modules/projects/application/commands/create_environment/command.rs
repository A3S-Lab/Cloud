use crate::modules::projects::domain::entities::Environment;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateEnvironment {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub name: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateEnvironment {
    type Output = ApplicationResult<CreateEnvironmentResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateEnvironmentResult {
    pub environment: Environment,
    pub replayed: bool,
}
