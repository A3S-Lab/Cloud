use crate::modules::edge::domain::DomainClaim;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{DomainClaimId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone)]
pub struct RevokeDomainClaim {
    pub organization_id: OrganizationId,
    pub claim_id: DomainClaimId,
    pub reason: String,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl std::fmt::Debug for RevokeDomainClaim {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RevokeDomainClaim")
            .field("organization_id", &self.organization_id)
            .field("claim_id", &self.claim_id)
            .field("reason", &"<redacted-reason>")
            .field("idempotency_key", &self.idempotency_key)
            .field("request_id", &self.request_id)
            .field("requested_at", &self.requested_at)
            .finish()
    }
}

impl Command for RevokeDomainClaim {
    type Output = ApplicationResult<RevokeDomainClaimResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevokeDomainClaimResult {
    pub claim: DomainClaim,
    pub replayed: bool,
}
