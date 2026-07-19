use crate::modules::edge::domain::DomainClaim;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainClaimResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub pattern: String,
    pub challenge_dns_name: String,
    pub challenge_value: String,
    pub state: String,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<DomainClaim> for DomainClaimResponse {
    fn from(claim: DomainClaim) -> Self {
        Self {
            id: claim.id.as_uuid(),
            organization_id: claim.organization_id.as_uuid(),
            project_id: claim.project_id.as_uuid(),
            environment_id: claim.environment_id.as_uuid(),
            pattern: claim.pattern.as_str().into(),
            challenge_dns_name: claim.challenge_dns_name,
            challenge_value: claim.challenge_value,
            state: claim.state.as_str().into(),
            failure: claim.failure,
            aggregate_version: claim.aggregate_version,
            created_at: claim.created_at,
            updated_at: claim.updated_at,
            verified_at: claim.verified_at,
            revoked_at: claim.revoked_at,
        }
    }
}
