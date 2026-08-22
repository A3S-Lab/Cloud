use crate::modules::identity::domain::entities::{
    RecipientContactRecord, RecipientContactVerification,
};
use crate::modules::shared_kernel::domain::{
    OrganizationId, RecipientContactVerificationId, Sha256Digest,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipientContactChanged {
    pub contact_id: Uuid,
    pub principal_id: Uuid,
    pub state: String,
    pub address_digest: Sha256Digest,
    pub contact_version: u64,
    pub challenge_id: Option<RecipientContactVerificationId>,
    pub signing_key_id: Option<String>,
    pub challenge_issued_at: Option<DateTime<Utc>>,
    pub challenge_expires_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl RecipientContactChanged {
    pub fn verification_requested(
        organization_id: OrganizationId,
        contact: &RecipientContactRecord,
        verification: &RecipientContactVerification,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            verification.id.as_uuid(),
            "identity.recipient-contact.verification-requested",
            organization_id,
            contact,
            Some(verification),
            verification.issued_at,
            correlation_id,
        )
    }

    pub fn verified(
        organization_id: OrganizationId,
        contact: &RecipientContactRecord,
        verification: &RecipientContactVerification,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            Uuid::now_v7(),
            "identity.recipient-contact.verified",
            organization_id,
            contact,
            Some(verification),
            contact.updated_at,
            correlation_id,
        )
    }

    pub fn revoked(
        organization_id: OrganizationId,
        contact: &RecipientContactRecord,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            Uuid::now_v7(),
            "identity.recipient-contact.revoked",
            organization_id,
            contact,
            None,
            contact.updated_at,
            correlation_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn envelope(
        event_id: Uuid,
        event_key: &str,
        organization_id: OrganizationId,
        contact: &RecipientContactRecord,
        verification: Option<&RecipientContactVerification>,
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            contact_id: contact.id.as_uuid(),
            principal_id: contact.principal_id.as_uuid(),
            state: contact.status.as_str().into(),
            address_digest: contact.address_digest.clone(),
            contact_version: contact.aggregate_version,
            challenge_id: verification.map(|value| value.id),
            signing_key_id: verification.map(|value| value.signing_key_id.as_str().to_owned()),
            challenge_issued_at: verification.map(|value| value.issued_at),
            challenge_expires_at: verification.map(|value| value.expires_at),
            verified_at: contact.verified_at,
            revoked_at: contact.revoked_at,
        };
        Ok(DomainEventEnvelope {
            event_id,
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: organization_id.as_uuid(),
            aggregate_id: contact.id.as_uuid(),
            aggregate_version: contact.aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::RecipientContact;
    use crate::modules::identity::domain::value_objects::{
        RecipientContactSigningKeyId, RecipientEmailAddress,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, RecipientContactId, RecipientContactVerificationId,
    };
    use chrono::Duration;

    #[test]
    fn event_evidence_excludes_mailbox_and_proof_material() {
        let now = Utc::now();
        let contact = RecipientContact::create(
            RecipientContactId::new(),
            PrincipalId::new(),
            RecipientEmailAddress::parse("private@example.com").expect("address"),
            now,
        )
        .expect("contact");
        let verification = RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            contact.id,
            contact.principal_id,
            contact.address.digest(),
            contact.aggregate_version,
            RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification");
        let event = RecipientContactChanged::verification_requested(
            OrganizationId::new(),
            &contact.record(),
            &verification,
            Uuid::now_v7(),
        )
        .expect("event");
        let encoded = serde_json::to_string(&event).expect("encoded event");
        assert!(!encoded.contains("private@example.com"));
        assert!(!encoded.contains("proof"));
        assert!(!encoded.contains("signature"));
    }
}
