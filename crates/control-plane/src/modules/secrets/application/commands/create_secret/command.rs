use crate::modules::secrets::application::{SecretMutationResult, SecretPlaintext};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug)]
pub struct CreateSecret {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub name: String,
    pub value: SecretPlaintext,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateSecret {
    type Output = ApplicationResult<SecretMutationResult>;
}
