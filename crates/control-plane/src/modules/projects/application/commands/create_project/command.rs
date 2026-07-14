use crate::modules::projects::domain::entities::Project;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateProject {
    pub organization_id: OrganizationId,
    pub name: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateProject {
    type Output = ApplicationResult<CreateProjectResult>;
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateProjectResult {
    pub project: Project,
    pub replayed: bool,
}
