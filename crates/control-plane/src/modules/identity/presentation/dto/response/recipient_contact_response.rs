use crate::modules::identity::application::{
    RecipientContactMutationResult, RecipientContactVerificationRequestResult,
};
use crate::modules::identity::domain::entities::RecipientContactRecord;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientContactResponse {
    pub id: Uuid,
    pub principal_id: Uuid,
    pub address_digest: String,
    pub address_hint: String,
    pub aggregate_version: u64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<RecipientContactRecord> for RecipientContactResponse {
    fn from(record: RecipientContactRecord) -> Self {
        Self {
            id: record.id.as_uuid(),
            principal_id: record.principal_id.as_uuid(),
            address_digest: record.address_digest.as_str().to_owned(),
            address_hint: record.address_hint,
            aggregate_version: record.aggregate_version,
            status: record.status.as_str().to_owned(),
            created_at: record.created_at,
            updated_at: record.updated_at,
            verified_at: record.verified_at,
            revoked_at: record.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipientContactMutationResponse {
    #[serde(flatten)]
    pub contact: RecipientContactResponse,
    pub replayed: bool,
}

impl From<RecipientContactMutationResult> for RecipientContactMutationResponse {
    fn from(result: RecipientContactMutationResult) -> Self {
        Self {
            contact: result.contact.into(),
            replayed: result.replayed,
        }
    }
}

impl From<RecipientContactVerificationRequestResult> for RecipientContactMutationResponse {
    fn from(result: RecipientContactVerificationRequestResult) -> Self {
        Self {
            contact: result.contact.into(),
            replayed: result.replayed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::{
        RecipientContactStatus, RecipientContactVerification,
    };
    use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
    use crate::modules::shared_kernel::domain::{
        PrincipalId, RecipientContactId, RecipientContactVerificationId, Sha256Digest,
    };
    use chrono::Duration;

    #[test]
    fn response_contains_only_the_redacted_contact_projection() {
        let now = Utc::now();
        let record = RecipientContactRecord {
            id: RecipientContactId::new(),
            principal_id: PrincipalId::new(),
            address_digest: Sha256Digest::from_bytes(b"private@example.test"),
            address_hint: "***@example.test".into(),
            aggregate_version: 1,
            status: RecipientContactStatus::Pending,
            created_at: now,
            updated_at: now,
            verified_at: None,
            revoked_at: None,
        };
        let verification = RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            record.id,
            record.principal_id,
            record.address_digest.clone(),
            record.aggregate_version,
            RecipientContactSigningKeyId::parse("recipient-contact-v1").expect("key"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification");
        let response =
            RecipientContactMutationResponse::from(RecipientContactVerificationRequestResult {
                contact: record,
                verification,
                replayed: false,
            });
        let encoded = serde_json::to_string(&response).expect("response");
        assert!(!encoded.contains("private@example.test"));
        assert!(!encoded.contains("challengeId"));
        assert!(!encoded.contains("signingKeyId"));
        assert!(encoded.contains("***@example.test"));
        assert!(encoded.contains("addressDigest"));
    }
}
