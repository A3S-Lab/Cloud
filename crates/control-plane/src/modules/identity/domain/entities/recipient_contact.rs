use crate::modules::identity::domain::entities::RecipientContactVerificationClaims;
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, PrincipalId, RecipientContactId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientContactStatus {
    Pending,
    Verified,
    Revoked,
}

impl RecipientContactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "verified" => Ok(Self::Verified),
            "revoked" => Ok(Self::Revoked),
            _ => Err("recipient contact status is invalid".into()),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RecipientContact {
    pub id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address: RecipientEmailAddress,
    pub aggregate_version: u64,
    pub status: RecipientContactStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for RecipientContact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecipientContact")
            .field("id", &self.id)
            .field("principal_id", &self.principal_id)
            .field("address", &"[REDACTED]")
            .field("aggregate_version", &self.aggregate_version)
            .field("status", &self.status)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .field("verified_at", &self.verified_at)
            .field("revoked_at", &self.revoked_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientContactRecord {
    pub id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address_digest: Sha256Digest,
    pub address_hint: String,
    pub aggregate_version: u64,
    pub status: RecipientContactStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl RecipientContact {
    pub fn create(
        id: RecipientContactId,
        principal_id: PrincipalId,
        address: RecipientEmailAddress,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if id.as_uuid().is_nil() || principal_id.as_uuid().is_nil() {
            return Err("recipient contact identifiers must not be nil".into());
        }
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            principal_id,
            address,
            aggregate_version: 1,
            status: RecipientContactStatus::Pending,
            created_at,
            updated_at: created_at,
            verified_at: None,
            revoked_at: None,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || self
                .verified_at
                .is_some_and(|value| value != canonical_timestamp(value) || value < self.created_at)
            || self
                .revoked_at
                .is_some_and(|value| value != canonical_timestamp(value) || value < self.created_at)
            || !matches!(
                (self.status, self.verified_at, self.revoked_at),
                (RecipientContactStatus::Pending, None, None)
                    | (RecipientContactStatus::Verified, Some(_), None)
                    | (RecipientContactStatus::Revoked, _, Some(_))
            )
        {
            return Err("recipient contact is invalid".into());
        }
        Ok(())
    }

    pub fn verify(
        &mut self,
        claims: &RecipientContactVerificationClaims,
        verified_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let verified_at = canonical_timestamp(verified_at);
        if self.status != RecipientContactStatus::Pending
            || claims.contact_id != self.id
            || claims.principal_id != self.principal_id
            || claims.address_digest != self.address.digest()
            || claims.contact_version != self.aggregate_version
            || verified_at < claims.issued_at
            || verified_at >= claims.expires_at
        {
            return Err("recipient contact verification does not match the pending contact".into());
        }
        self.aggregate_version += 1;
        self.status = RecipientContactStatus::Verified;
        let verified_at = verified_at.max(self.updated_at);
        self.verified_at = Some(verified_at);
        self.updated_at = verified_at;
        self.validate()
    }

    pub fn revoke(&mut self, revoked_at: DateTime<Utc>) -> bool {
        if self.status == RecipientContactStatus::Revoked {
            return false;
        }
        let revoked_at = canonical_timestamp(revoked_at).max(self.updated_at);
        self.aggregate_version += 1;
        self.status = RecipientContactStatus::Revoked;
        self.revoked_at = Some(revoked_at);
        self.updated_at = revoked_at;
        true
    }

    pub fn record(&self) -> RecipientContactRecord {
        RecipientContactRecord {
            id: self.id,
            principal_id: self.principal_id,
            address_digest: self.address.digest(),
            address_hint: self.address.redacted_hint(),
            aggregate_version: self.aggregate_version,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            verified_at: self.verified_at,
            revoked_at: self.revoked_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::entities::RecipientContactVerification;
    use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
    use crate::modules::shared_kernel::domain::RecipientContactVerificationId;
    use chrono::Duration;

    #[test]
    fn contact_verification_and_revocation_are_versioned_and_terminal() {
        let now = canonical_timestamp(Utc::now());
        let mut contact = RecipientContact::create(
            RecipientContactId::new(),
            PrincipalId::new(),
            RecipientEmailAddress::parse("alerts@example.com").expect("address"),
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
        contact
            .verify(&verification.claims(), now + Duration::minutes(1))
            .expect("verified contact");
        assert_eq!(contact.status, RecipientContactStatus::Verified);
        assert_eq!(contact.aggregate_version, 2);
        assert!(contact.revoke(now + Duration::minutes(2)));
        assert_eq!(contact.status, RecipientContactStatus::Revoked);
        assert_eq!(contact.aggregate_version, 3);
        assert!(!contact.revoke(now + Duration::minutes(3)));
    }

    #[test]
    fn record_is_redacted_and_never_serializes_the_mailbox() {
        let contact = RecipientContact::create(
            RecipientContactId::new(),
            PrincipalId::new(),
            RecipientEmailAddress::parse("private@example.com").expect("address"),
            Utc::now(),
        )
        .expect("contact");
        let encoded = serde_json::to_string(&contact.record()).expect("record");
        assert!(!encoded.contains("private@example.com"));
        assert!(!format!("{contact:?}").contains("private@example.com"));
    }
}
