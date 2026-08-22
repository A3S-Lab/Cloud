use crate::modules::identity::domain::entities::{
    RecipientContactVerificationDeliveryFact, RecipientContactVerificationDeliveryOutcome,
    RecipientContactVerificationDeliveryRecord, RecipientContactVerificationDeliveryReservation,
    RecipientContactVerificationDeliveryStatus,
};
use crate::modules::shared_kernel::domain::{RecipientContactVerificationId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipientContactVerificationDeliveryAdmission {
    Reserved(RecipientContactVerificationDeliveryReservation),
    Deferred { lease_expires_at: DateTime<Utc> },
    Terminal(RecipientContactVerificationDeliveryStatus),
    InvalidFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientContactVerificationDispatchStart {
    Authorized,
    Deferred,
    Terminal(RecipientContactVerificationDeliveryStatus),
}

#[async_trait]
pub trait IRecipientContactVerificationDeliveryRepository: Send + Sync {
    async fn reserve_recipient_contact_verification_delivery(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryAdmission, RepositoryError>;

    async fn start_recipient_contact_verification_dispatch(
        &self,
        fact: &RecipientContactVerificationDeliveryFact,
        fence_token: Uuid,
        started_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDispatchStart, RepositoryError>;

    async fn settle_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
        fence_token: Uuid,
        outcome: RecipientContactVerificationDeliveryOutcome,
        settled_at: DateTime<Utc>,
    ) -> Result<RecipientContactVerificationDeliveryRecord, RepositoryError>;

    async fn find_recipient_contact_verification_delivery(
        &self,
        verification_id: RecipientContactVerificationId,
    ) -> Result<Option<RecipientContactVerificationDeliveryRecord>, RepositoryError>;
}
