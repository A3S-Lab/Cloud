use crate::modules::identity::application::RecipientContactVerificationRequestResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use std::fmt;
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct BeginRecipientContactVerification {
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub address: Zeroizing<String>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl fmt::Debug for BeginRecipientContactVerification {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BeginRecipientContactVerification")
            .field("organization_id", &self.organization_id)
            .field("actor_principal_id", &self.actor_principal_id)
            .field("address", &"[REDACTED]")
            .field("idempotency_key", &self.idempotency_key)
            .field("request_id", &self.request_id)
            .finish()
    }
}

impl Command for BeginRecipientContactVerification {
    type Output = ApplicationResult<RecipientContactVerificationRequestResult>;
}
