use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct OutboundVerifiedRecipientContact {
    pub id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address: String,
    pub aggregate_version: u64,
    pub verified_at: DateTime<Utc>,
}

impl fmt::Debug for OutboundVerifiedRecipientContact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundVerifiedRecipientContact")
            .field("id", &self.id)
            .field("principal_id", &self.principal_id)
            .field("address", &"[REDACTED]")
            .field("aggregate_version", &self.aggregate_version)
            .field("verified_at", &self.verified_at)
            .finish()
    }
}

/// Consumer-owned boundary for verified recipient-contact facts used by outbound
/// subscription admission and SMTP dispatch. Identity remains the sole contact
/// authority behind the adapter.
#[async_trait]
pub trait IOutboundRecipientContactAccess: Send + Sync {
    async fn resolve_verified(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        contact_id: RecipientContactId,
    ) -> ApplicationResult<Option<OutboundVerifiedRecipientContact>>;
}
