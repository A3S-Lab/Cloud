use crate::modules::secrets::application::SecretMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, SecretId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeSecretVersion {
    pub organization_id: OrganizationId,
    pub secret_id: SecretId,
    pub version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeSecretVersion {
    type Output = ApplicationResult<SecretMutationResult>;
}
