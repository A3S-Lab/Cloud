use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, PrincipalId, RecipientContactId, RecipientContactVerificationId,
    Sha256Digest,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const MIN_RECIPIENT_CONTACT_VERIFICATION_LIFETIME: Duration = Duration::minutes(1);
pub const MAX_RECIPIENT_CONTACT_VERIFICATION_LIFETIME: Duration = Duration::minutes(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientContactVerificationStatus {
    Pending,
    Consumed,
    Invalidated,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipientContactVerificationClaims {
    pub contact_id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address_digest: Sha256Digest,
    pub contact_version: u64,
    pub challenge_id: RecipientContactVerificationId,
    pub signing_key_id: RecipientContactSigningKeyId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl RecipientContactVerificationClaims {
    pub fn validate(&self) -> Result<(), String> {
        if self.contact_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.challenge_id.as_uuid().is_nil()
            || self.contact_version == 0
            || self.issued_at != canonical_timestamp(self.issued_at)
            || self.expires_at != canonical_timestamp(self.expires_at)
            || self.expires_at < self.issued_at + MIN_RECIPIENT_CONTACT_VERIFICATION_LIFETIME
            || self.expires_at > self.issued_at + MAX_RECIPIENT_CONTACT_VERIFICATION_LIFETIME
        {
            return Err("recipient contact verification claims are invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientContactVerification {
    pub id: RecipientContactVerificationId,
    pub contact_id: RecipientContactId,
    pub principal_id: PrincipalId,
    pub address_digest: Sha256Digest,
    pub contact_version: u64,
    pub signing_key_id: RecipientContactSigningKeyId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub invalidated_at: Option<DateTime<Utc>>,
}

impl RecipientContactVerification {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: RecipientContactVerificationId,
        contact_id: RecipientContactId,
        principal_id: PrincipalId,
        address_digest: Sha256Digest,
        contact_version: u64,
        signing_key_id: RecipientContactSigningKeyId,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            id,
            contact_id,
            principal_id,
            address_digest,
            contact_version,
            signing_key_id,
            issued_at: canonical_timestamp(issued_at),
            expires_at: canonical_timestamp(expires_at),
            consumed_at: None,
            invalidated_at: None,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.claims().validate()?;
        if self.id != self.claims().challenge_id
            || self.consumed_at.zip(self.invalidated_at).is_some()
            || self.consumed_at.is_some_and(|value| {
                value != canonical_timestamp(value)
                    || value < self.issued_at
                    || value >= self.expires_at
            })
            || self
                .invalidated_at
                .is_some_and(|value| value != canonical_timestamp(value) || value < self.issued_at)
        {
            return Err("recipient contact verification is invalid".into());
        }
        Ok(())
    }

    pub fn status_at(&self, now: DateTime<Utc>) -> RecipientContactVerificationStatus {
        if self.consumed_at.is_some() {
            RecipientContactVerificationStatus::Consumed
        } else if self.invalidated_at.is_some() {
            RecipientContactVerificationStatus::Invalidated
        } else if canonical_timestamp(now) >= self.expires_at {
            RecipientContactVerificationStatus::Expired
        } else {
            RecipientContactVerificationStatus::Pending
        }
    }

    pub fn consume(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        let now = canonical_timestamp(now);
        if self.status_at(now) != RecipientContactVerificationStatus::Pending {
            return Err("recipient contact verification is not pending".into());
        }
        self.consumed_at = Some(now);
        self.validate()
    }

    pub fn invalidate(&mut self, now: DateTime<Utc>) -> bool {
        let now = canonical_timestamp(now);
        if self.consumed_at.is_some() || self.invalidated_at.is_some() {
            return false;
        }
        self.invalidated_at = Some(now.max(self.issued_at));
        true
    }

    pub fn claims(&self) -> RecipientContactVerificationClaims {
        RecipientContactVerificationClaims {
            contact_id: self.contact_id,
            principal_id: self.principal_id,
            address_digest: self.address_digest.clone(),
            contact_version: self.contact_version,
            challenge_id: self.id,
            signing_key_id: self.signing_key_id.clone(),
            issued_at: self.issued_at,
            expires_at: self.expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verification(now: DateTime<Utc>) -> RecipientContactVerification {
        RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            RecipientContactId::new(),
            PrincipalId::new(),
            Sha256Digest::from_bytes(b"address"),
            1,
            RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification")
    }

    #[test]
    fn verification_is_bounded_single_use_and_invalidatable() {
        let now = canonical_timestamp(Utc::now());
        let mut value = verification(now);
        value.consume(now + Duration::minutes(1)).expect("consume");
        assert_eq!(
            value.status_at(now + Duration::minutes(2)),
            RecipientContactVerificationStatus::Consumed
        );
        assert!(value.consume(now + Duration::minutes(2)).is_err());
        assert!(!value.invalidate(now + Duration::minutes(2)));

        let mut reissued = verification(now);
        assert!(reissued.invalidate(now + Duration::seconds(1)));
        assert!(reissued.consume(now + Duration::minutes(1)).is_err());
    }

    #[test]
    fn verification_lifetime_is_strictly_bounded() {
        let now = canonical_timestamp(Utc::now());
        let common = (
            RecipientContactVerificationId::new(),
            RecipientContactId::new(),
            PrincipalId::new(),
            Sha256Digest::from_bytes(b"address"),
            RecipientContactSigningKeyId::parse("contact-v1").expect("key"),
        );
        for expires_at in [now + Duration::seconds(59), now + Duration::minutes(31)] {
            assert!(RecipientContactVerification::create(
                common.0,
                common.1,
                common.2,
                common.3.clone(),
                1,
                common.4.clone(),
                now,
                expires_at,
            )
            .is_err());
        }
    }
}
