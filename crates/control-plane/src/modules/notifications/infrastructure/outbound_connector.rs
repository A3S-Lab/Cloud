use crate::modules::connectors::ConnectorExecutionRequest;
use crate::modules::notifications::domain::{
    IOutboundNotificationRequestAdapter, OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationRequestError,
};
use chrono::SecondsFormat;
use serde::Serialize;
use uuid::Uuid;

/// Builds the signed-webhook request shape without owning execution or retry.
#[derive(Debug, Clone, Copy, Default)]
pub struct SignedWebhookNotificationAdapter;

impl SignedWebhookNotificationAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl IOutboundNotificationRequestAdapter for SignedWebhookNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SignedWebhook
    }

    fn build_request(
        &self,
        delivery: &OutboundNotificationDelivery,
        attempt_id: Uuid,
    ) -> Result<ConnectorExecutionRequest, OutboundNotificationRequestError> {
        validate_delivery(delivery, self.channel())?;
        let body = delivery
            .canonical_payload()
            .map_err(|_| OutboundNotificationRequestError::Rejected)?;
        // This value must remain stable when the same fenced Connector attempt
        // is replayed after an acknowledgement or process-loss boundary.
        let occurred_at = delivery
            .occurred_at()
            .to_rfc3339_opts(SecondsFormat::Micros, true);
        let signing_input = webhook_signing_input(&occurred_at, delivery.id(), &body);
        delivery_request(delivery, attempt_id, body)?
            .with_header("x-a3s-notification-occurred-at", occurred_at)
            .map_err(|_| OutboundNotificationRequestError::Rejected)?
            .with_signing_input(signing_input)
            .map_err(|_| OutboundNotificationRequestError::Rejected)
    }
}

/// Builds the Slack-compatible request shape without owning execution or retry.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlackCompatibleNotificationAdapter;

impl SlackCompatibleNotificationAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl IOutboundNotificationRequestAdapter for SlackCompatibleNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SlackCompatible
    }

    fn build_request(
        &self,
        delivery: &OutboundNotificationDelivery,
        attempt_id: Uuid,
    ) -> Result<ConnectorExecutionRequest, OutboundNotificationRequestError> {
        validate_delivery(delivery, self.channel())?;
        let body = crate::modules::shared_kernel::domain::canonical_json_bounded(
            &SlackCompatiblePayload {
                text: format!(
                    "[{}] {}\n{}",
                    delivery.severity().as_str(),
                    delivery.title(),
                    delivery.body()
                ),
            },
            16 * 1024,
            "Slack-compatible notification payload",
        )
        .map_err(|_| OutboundNotificationRequestError::Rejected)?;
        delivery_request(delivery, attempt_id, body)
    }
}

#[derive(Serialize)]
struct SlackCompatiblePayload {
    text: String,
}

fn validate_delivery(
    delivery: &OutboundNotificationDelivery,
    channel: OutboundNotificationChannel,
) -> Result<(), OutboundNotificationRequestError> {
    delivery
        .validate()
        .map_err(|_| OutboundNotificationRequestError::Rejected)?;
    if delivery.channel() != channel || channel == OutboundNotificationChannel::Smtp {
        return Err(OutboundNotificationRequestError::Rejected);
    }
    Ok(())
}

fn delivery_request(
    delivery: &OutboundNotificationDelivery,
    attempt_id: Uuid,
    body: Vec<u8>,
) -> Result<ConnectorExecutionRequest, OutboundNotificationRequestError> {
    let revision_id = delivery
        .target_revision_id()
        .ok_or(OutboundNotificationRequestError::Rejected)?;
    ConnectorExecutionRequest::new(revision_id, attempt_id, "application/json", body)
        .and_then(|request| {
            request.with_header("x-a3s-notification-delivery-id", delivery.id().to_string())
        })
        .map_err(|_| OutboundNotificationRequestError::Rejected)
}

fn webhook_signing_input(occurred_at: &str, delivery_id: Uuid, body: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(occurred_at.len() + body.len() + 80);
    input.extend_from_slice(b"v1\n");
    input.extend_from_slice(occurred_at.as_bytes());
    input.extend_from_slice(b"\n");
    input.extend_from_slice(delivery_id.to_string().as_bytes());
    input.extend_from_slice(b"\n");
    input.extend_from_slice(body);
    input
}

#[cfg(test)]
#[path = "outbound_connector_tests.rs"]
mod tests;
