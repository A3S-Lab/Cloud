use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationStatus,
};
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, RecipientContactVerificationId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientContactVerificationDeliveryStatus {
    Reserved,
    Dispatching,
    Delivered,
    Rejected,
    Indeterminate,
    Obsolete,
}

impl RecipientContactVerificationDeliveryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Dispatching => "dispatching",
            Self::Delivered => "delivered",
            Self::Rejected => "rejected",
            Self::Indeterminate => "indeterminate",
            Self::Obsolete => "obsolete",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "dispatching" => Ok(Self::Dispatching),
            "delivered" => Ok(Self::Delivered),
            "rejected" => Ok(Self::Rejected),
            "indeterminate" => Ok(Self::Indeterminate),
            "obsolete" => Ok(Self::Obsolete),
            _ => Err("recipient contact verification delivery status is invalid".into()),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Delivered | Self::Rejected | Self::Indeterminate | Self::Obsolete
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientContactVerificationDeliveryOutcome {
    Delivered,
    Rejected,
    Indeterminate,
}

impl RecipientContactVerificationDeliveryOutcome {
    pub const fn status(self) -> RecipientContactVerificationDeliveryStatus {
        match self {
            Self::Delivered => RecipientContactVerificationDeliveryStatus::Delivered,
            Self::Rejected => RecipientContactVerificationDeliveryStatus::Rejected,
            Self::Indeterminate => RecipientContactVerificationDeliveryStatus::Indeterminate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientContactVerificationDeliveryFact {
    pub organization_id: OrganizationId,
    pub verification: RecipientContactVerification,
}

impl RecipientContactVerificationDeliveryFact {
    pub fn validate(&self) -> Result<(), String> {
        self.verification.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.verification.status_at(self.verification.issued_at)
                != RecipientContactVerificationStatus::Pending
        {
            return Err("recipient contact verification delivery fact is invalid".into());
        }
        Ok(())
    }

    pub const fn id(&self) -> RecipientContactVerificationId {
        self.verification.id
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RecipientContactVerificationDeliveryReservation {
    pub verification: RecipientContactVerification,
    pub address: RecipientEmailAddress,
    pub fence_token: Uuid,
    pub lease_expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for RecipientContactVerificationDeliveryReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecipientContactVerificationDeliveryReservation")
            .field("verification", &self.verification)
            .field("address", &"[REDACTED]")
            .field("fence_token", &self.fence_token)
            .field("lease_expires_at", &self.lease_expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientContactVerificationDeliveryRecord {
    pub verification_id: RecipientContactVerificationId,
    pub status: RecipientContactVerificationDeliveryStatus,
    pub fence_token: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub dispatch_started_at: Option<DateTime<Utc>>,
    pub settled_at: Option<DateTime<Utc>>,
}

impl RecipientContactVerificationDeliveryRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        verification_id: RecipientContactVerificationId,
        status: RecipientContactVerificationDeliveryStatus,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
        dispatch_started_at: Option<DateTime<Utc>>,
        settled_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let value = Self {
            verification_id,
            status,
            fence_token,
            reserved_at,
            lease_expires_at,
            dispatch_started_at,
            settled_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let reserved_at = canonical_timestamp(self.reserved_at);
        let lease_expires_at = canonical_timestamp(self.lease_expires_at);
        let dispatch_started_at = self.dispatch_started_at.map(canonical_timestamp);
        let settled_at = self.settled_at.map(canonical_timestamp);
        let common_valid = !self.verification_id.as_uuid().is_nil()
            && !self.fence_token.is_nil()
            && self.reserved_at == reserved_at
            && self.lease_expires_at == lease_expires_at
            && self.lease_expires_at > self.reserved_at
            && self.dispatch_started_at == dispatch_started_at
            && self.settled_at == settled_at;
        let state_valid = match self.status {
            RecipientContactVerificationDeliveryStatus::Reserved => {
                self.dispatch_started_at.is_none() && self.settled_at.is_none()
            }
            RecipientContactVerificationDeliveryStatus::Dispatching => {
                self.dispatch_started_at.is_some_and(|started| {
                    started >= self.reserved_at && started < self.lease_expires_at
                }) && self.settled_at.is_none()
            }
            RecipientContactVerificationDeliveryStatus::Delivered
            | RecipientContactVerificationDeliveryStatus::Rejected
            | RecipientContactVerificationDeliveryStatus::Indeterminate => self
                .dispatch_started_at
                .zip(self.settled_at)
                .is_some_and(|(started, settled)| {
                    started >= self.reserved_at
                        && started < self.lease_expires_at
                        && settled >= started
                }),
            RecipientContactVerificationDeliveryStatus::Obsolete => {
                self.dispatch_started_at.is_none()
                    && self
                        .settled_at
                        .is_some_and(|settled| settled >= self.reserved_at)
            }
        };
        if !common_valid || !state_valid {
            return Err("recipient contact verification delivery record is invalid".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
    use crate::modules::shared_kernel::domain::{PrincipalId, RecipientContactId, Sha256Digest};
    use chrono::Duration;

    fn verification(now: DateTime<Utc>) -> RecipientContactVerification {
        RecipientContactVerification::create(
            RecipientContactVerificationId::new(),
            RecipientContactId::new(),
            PrincipalId::new(),
            Sha256Digest::from_bytes(b"mailbox"),
            1,
            RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
            now,
            now + Duration::minutes(10),
        )
        .expect("verification")
    }

    #[test]
    fn delivery_states_enforce_the_one_way_dispatch_boundary() {
        let now = canonical_timestamp(Utc::now());
        let verification = verification(now);
        let fence = Uuid::now_v7();
        assert!(RecipientContactVerificationDeliveryFact {
            organization_id: OrganizationId::new(),
            verification: verification.clone(),
        }
        .validate()
        .is_ok());
        assert!(RecipientContactVerificationDeliveryRecord::restore(
            verification.id,
            RecipientContactVerificationDeliveryStatus::Reserved,
            fence,
            now,
            now + Duration::minutes(1),
            None,
            None,
        )
        .is_ok());
        assert!(RecipientContactVerificationDeliveryRecord::restore(
            verification.id,
            RecipientContactVerificationDeliveryStatus::Delivered,
            fence,
            now,
            now + Duration::minutes(1),
            None,
            Some(now + Duration::seconds(2)),
        )
        .is_err());
    }
}
