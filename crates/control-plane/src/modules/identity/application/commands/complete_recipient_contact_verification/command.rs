use crate::modules::identity::application::RecipientContactMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use a3s_boot::Command;
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct CompleteRecipientContactVerification {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub contact_id: RecipientContactId,
    pub proof: Zeroizing<String>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl fmt::Debug for CompleteRecipientContactVerification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompleteRecipientContactVerification")
            .field("organization_id", &self.organization_id)
            .field("actor_principal_id", &self.actor_principal_id)
            .field("contact_id", &self.contact_id)
            .field("proof", &"[REDACTED]")
            .field("idempotency_key", &self.idempotency_key)
            .field("request_id", &self.request_id)
            .finish()
    }
}

impl Command for CompleteRecipientContactVerification {
    type Output = ApplicationResult<RecipientContactMutationResult>;
}
