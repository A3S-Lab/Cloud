use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use crate::modules::shared_kernel::domain::RecipientContactVerificationId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

pub struct RecipientContactVerificationDeliveryRequest {
    pub verification_id: RecipientContactVerificationId,
    pub address: RecipientEmailAddress,
    pub proof: Zeroizing<String>,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for RecipientContactVerificationDeliveryRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecipientContactVerificationDeliveryRequest")
            .field("verification_id", &self.verification_id)
            .field("address", &"[REDACTED]")
            .field("proof", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientContactVerificationProviderOutcome {
    Delivered,
    Rejected,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecipientContactVerificationDeliveryPreparationError {
    #[error("recipient contact verification delivery provider is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait IPreparedRecipientContactVerificationDelivery: Send {
    async fn deliver(
        self: Box<Self>,
        request: RecipientContactVerificationDeliveryRequest,
    ) -> RecipientContactVerificationProviderOutcome;
}

#[async_trait]
pub trait IRecipientContactVerificationDeliveryService: Send + Sync {
    async fn prepare(
        &self,
    ) -> Result<
        Box<dyn IPreparedRecipientContactVerificationDelivery>,
        RecipientContactVerificationDeliveryPreparationError,
    >;
}
