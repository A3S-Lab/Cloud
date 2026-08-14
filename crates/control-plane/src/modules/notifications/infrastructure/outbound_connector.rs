use crate::modules::connectors::{
    ConnectorExecutionError, ConnectorExecutionRequest, IConnectorExecutionPort,
};
use crate::modules::notifications::domain::{
    IOutboundNotificationAdapter, OutboundNotificationChannel, OutboundNotificationDelivery,
    OutboundNotificationDeliveryError, OutboundNotificationDeliveryReceipt,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, ConnectorRevisionId};
use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

pub struct SignedWebhookNotificationAdapter {
    connector: Arc<dyn IConnectorExecutionPort>,
    target_revision_id: ConnectorRevisionId,
}

impl SignedWebhookNotificationAdapter {
    pub fn new(
        target_revision_id: ConnectorRevisionId,
        connector: Arc<dyn IConnectorExecutionPort>,
    ) -> Result<Self, String> {
        validate_target(target_revision_id)?;
        Ok(Self {
            connector,
            target_revision_id,
        })
    }
}

impl fmt::Debug for SignedWebhookNotificationAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SignedWebhookNotificationAdapter")
            .field("target_revision_id", &self.target_revision_id)
            .field("connector", &"opaque execution port")
            .finish()
    }
}

#[async_trait]
impl IOutboundNotificationAdapter for SignedWebhookNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SignedWebhook
    }

    fn target_revision_id(&self) -> ConnectorRevisionId {
        self.target_revision_id
    }

    async fn deliver(
        &self,
        delivery: &OutboundNotificationDelivery,
        attempt_id: Uuid,
    ) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError> {
        validate_delivery(delivery, self.channel(), self.target_revision_id)?;
        let body = delivery
            .canonical_payload()
            .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
        let timestamp =
            canonical_timestamp(Utc::now()).to_rfc3339_opts(SecondsFormat::Micros, true);
        let signing_input = webhook_signing_input(&timestamp, delivery.id(), &body);
        let request = delivery_request(self.target_revision_id, delivery.id(), attempt_id, body)?
            .with_header("x-a3s-notification-timestamp", timestamp)
            .map_err(|_| OutboundNotificationDeliveryError::Rejected)?
            .with_signing_input(signing_input)
            .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
        execute_connector(&*self.connector, delivery.id(), request).await
    }
}

pub struct SlackCompatibleNotificationAdapter {
    connector: Arc<dyn IConnectorExecutionPort>,
    target_revision_id: ConnectorRevisionId,
}

impl SlackCompatibleNotificationAdapter {
    pub fn new(
        target_revision_id: ConnectorRevisionId,
        connector: Arc<dyn IConnectorExecutionPort>,
    ) -> Result<Self, String> {
        validate_target(target_revision_id)?;
        Ok(Self {
            connector,
            target_revision_id,
        })
    }
}

impl fmt::Debug for SlackCompatibleNotificationAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SlackCompatibleNotificationAdapter")
            .field("target_revision_id", &self.target_revision_id)
            .field("connector", &"opaque execution port")
            .finish()
    }
}

#[async_trait]
impl IOutboundNotificationAdapter for SlackCompatibleNotificationAdapter {
    fn channel(&self) -> OutboundNotificationChannel {
        OutboundNotificationChannel::SlackCompatible
    }

    fn target_revision_id(&self) -> ConnectorRevisionId {
        self.target_revision_id
    }

    async fn deliver(
        &self,
        delivery: &OutboundNotificationDelivery,
        attempt_id: Uuid,
    ) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError> {
        validate_delivery(delivery, self.channel(), self.target_revision_id)?;
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
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
        let request = delivery_request(self.target_revision_id, delivery.id(), attempt_id, body)?;
        execute_connector(&*self.connector, delivery.id(), request).await
    }
}

#[derive(Serialize)]
struct SlackCompatiblePayload {
    text: String,
}

fn validate_target(target_revision_id: ConnectorRevisionId) -> Result<(), String> {
    if target_revision_id.as_uuid().is_nil() {
        return Err("outbound notification target revision must not be nil".into());
    }
    Ok(())
}

fn validate_delivery(
    delivery: &OutboundNotificationDelivery,
    channel: OutboundNotificationChannel,
    target_revision_id: ConnectorRevisionId,
) -> Result<(), OutboundNotificationDeliveryError> {
    delivery
        .validate()
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)?;
    if delivery.channel() != channel || delivery.target_revision_id() != target_revision_id {
        return Err(OutboundNotificationDeliveryError::Rejected);
    }
    Ok(())
}

fn delivery_request(
    target_revision_id: ConnectorRevisionId,
    delivery_id: Uuid,
    attempt_id: Uuid,
    body: Vec<u8>,
) -> Result<ConnectorExecutionRequest, OutboundNotificationDeliveryError> {
    ConnectorExecutionRequest::new(target_revision_id, attempt_id, "application/json", body)
        .and_then(|request| {
            request.with_header("x-a3s-notification-delivery-id", delivery_id.to_string())
        })
        .map_err(|_| OutboundNotificationDeliveryError::Rejected)
}

async fn execute_connector(
    connector: &dyn IConnectorExecutionPort,
    delivery_id: Uuid,
    request: ConnectorExecutionRequest,
) -> Result<OutboundNotificationDeliveryReceipt, OutboundNotificationDeliveryError> {
    let receipt = connector
        .execute(&request)
        .await
        .map_err(map_connector_error)?;
    if receipt.connector_revision_id() != request.connector_revision_id()
        || receipt.attempt_id() != request.attempt_id()
    {
        return Err(OutboundNotificationDeliveryError::Rejected);
    }
    Ok(OutboundNotificationDeliveryReceipt {
        delivery_id,
        attempt_id: request.attempt_id(),
        target_revision_id: request.connector_revision_id(),
        accepted_at: receipt.accepted_at(),
    })
}

fn map_connector_error(error: ConnectorExecutionError) -> OutboundNotificationDeliveryError {
    match error {
        ConnectorExecutionError::Retryable { retry_after } => {
            OutboundNotificationDeliveryError::Retryable { retry_after }
        }
        ConnectorExecutionError::Rejected => OutboundNotificationDeliveryError::Rejected,
    }
}

fn webhook_signing_input(timestamp: &str, delivery_id: Uuid, body: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(timestamp.len() + body.len() + 80);
    input.extend_from_slice(b"v1\n");
    input.extend_from_slice(timestamp.as_bytes());
    input.extend_from_slice(b"\n");
    input.extend_from_slice(delivery_id.to_string().as_bytes());
    input.extend_from_slice(b"\n");
    input.extend_from_slice(body);
    input
}

#[cfg(test)]
#[path = "outbound_connector_tests.rs"]
mod tests;
