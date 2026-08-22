use crate::modules::identity::application::RecipientContactMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeRecipientContact {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
    pub expected_version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeRecipientContact {
    type Output = ApplicationResult<RecipientContactMutationResult>;
}
