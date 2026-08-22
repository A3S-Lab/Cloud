use super::OutboundNotificationDelivery;
use crate::modules::identity::domain::value_objects::RecipientEmailAddress;
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundNotificationSmtpPreparationError {
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundNotificationSmtpProviderOutcome {
    Accepted,
    Rejected,
    Retryable,
    Indeterminate,
}

#[async_trait]
pub trait IPreparedOutboundNotificationSmtpDelivery: Send {
    async fn deliver(self: Box<Self>) -> OutboundNotificationSmtpProviderOutcome;
}

#[async_trait]
pub trait IOutboundNotificationSmtpDeliveryService: Send + Sync {
    async fn prepare(
        &self,
        delivery: &OutboundNotificationDelivery,
        address: RecipientEmailAddress,
    ) -> Result<
        Box<dyn IPreparedOutboundNotificationSmtpDelivery>,
        OutboundNotificationSmtpPreparationError,
    >;
}
