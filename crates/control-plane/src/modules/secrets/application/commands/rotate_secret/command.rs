use crate::modules::secrets::application::{SecretMutationResult, SecretPlaintext};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, SecretId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug)]
pub struct RotateSecret {
    pub organization_id: OrganizationId,
    pub secret_id: SecretId,
    pub value: SecretPlaintext,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RotateSecret {
    type Output = ApplicationResult<SecretMutationResult>;
}
