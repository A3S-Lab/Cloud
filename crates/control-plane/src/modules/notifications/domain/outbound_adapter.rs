use super::{OutboundNotificationChannel, OutboundNotificationDelivery};
use crate::modules::connectors::ConnectorExecutionRequest;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OutboundNotificationRequestError {
    #[error("outbound notification request is invalid or unsupported")]
    Rejected,
}

/// Channel-specific, side-effect-free request construction.
///
/// The durable A3S Event consumer owns acknowledgement and asks the fenced
/// Connector application service to execute the returned request. Adapters
/// cannot issue network calls or implement retry policy.
pub trait IOutboundNotificationRequestAdapter: Send + Sync {
    fn channel(&self) -> OutboundNotificationChannel;

    fn build_request(
        &self,
        delivery: &OutboundNotificationDelivery,
        attempt_id: Uuid,
    ) -> Result<ConnectorExecutionRequest, OutboundNotificationRequestError>;
}
