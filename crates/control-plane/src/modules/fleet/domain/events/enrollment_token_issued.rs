use crate::modules::fleet::domain::entities::EnrollmentToken;
use crate::modules::shared_kernel::domain::{EnrollmentTokenId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentTokenIssued {
    pub organization_id: OrganizationId,
    pub enrollment_token_id: EnrollmentTokenId,
    pub name: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl EnrollmentTokenIssued {
    pub fn envelope(
        token: &EnrollmentToken,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "fleet.enrollment-token.issued".into(),
            schema_version: 1,
            organization_id: token.organization_id.as_uuid(),
            aggregate_id: token.id.as_uuid(),
            aggregate_version: token.aggregate_version,
            occurred_at: token.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: token.organization_id,
                enrollment_token_id: token.id,
                name: token.name.clone(),
                expires_at: token.expires_at,
            })?,
        })
    }
}
