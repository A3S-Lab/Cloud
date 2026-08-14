use super::{OutboundNotificationChannel, OutboundNotificationDelivery};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutboundNotificationDeliveryError {
    #[error("outbound notification provider is temporarily unavailable")]
    Retryable { retry_after: Option<Duration> },
    #[error("outbound notification provider rejected the delivery")]
    Rejected,
}

impl OutboundNotificationDeliveryError {
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }

    pub const fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Retryable { retry_after } => *retry_after,
            Self::Rejected => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundNotificationDeliveryReceipt {
    pub delivery_id: Uuid,
    pub target_revision_id: Uuid,
    pub accepted_at: DateTime<Utc>,
}

#[async_trait]
pub trait IOutboundNotificationAdapter: Send + Sync {
    fn channel(&self) -> OutboundNotificationChannel;

    fn target_revision_id(&self) -> Uuid;

    /// Performs exactly one provider attempt. Retry, backoff, rate policy, and durable
    /// acknowledgement belong to the shared A3S Event consumer boundary.
    async fn deliver(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError>;
}
