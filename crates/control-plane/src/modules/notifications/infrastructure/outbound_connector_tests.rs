use super::*;
use crate::modules::connectors::{ConnectorExecutionReceipt, ConnectorExecutionRequest};
use crate::modules::notifications::domain::{
    Notification, NotificationScope, NotificationSeverity,
};
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use chrono::Utc;
use std::sync::Mutex;

struct RecordingConnector {
    requests: Mutex<Vec<ConnectorExecutionRequest>>,
    error: Option<ConnectorExecutionError>,
    receipt_revision_id: Option<ConnectorRevisionId>,
}

impl RecordingConnector {
    fn accepting() -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            error: None,
            receipt_revision_id: None,
        }
    }

    fn failing(error: ConnectorExecutionError) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            error: Some(error),
            receipt_revision_id: None,
        }
    }

    fn drifting(receipt_revision_id: ConnectorRevisionId) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            error: None,
            receipt_revision_id: Some(receipt_revision_id),
        }
    }

    fn requests(&self) -> Vec<ConnectorExecutionRequest> {
        self.requests.lock().expect("request lock").clone()
    }
}

#[async_trait]
impl IConnectorExecutionPort for RecordingConnector {
    async fn execute(
        &self,
        request: &ConnectorExecutionRequest,
    ) -> Result<ConnectorExecutionReceipt, ConnectorExecutionError> {
        self.requests
            .lock()
            .expect("request lock")
            .push(request.clone());
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        ConnectorExecutionReceipt::accepted(
            self.receipt_revision_id
                .unwrap_or_else(|| request.connector_revision_id()),
            request.attempt_id(),
            Utc::now(),
            204,
            None,
            Vec::new(),
        )
    }
}

fn delivery(
    channel: OutboundNotificationChannel,
    target_revision_id: ConnectorRevisionId,
) -> OutboundNotificationDelivery {
    let now = Utc::now();
    let notification = Notification::project(
        OrganizationId::new(),
        PrincipalId::new(),
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        2,
        Uuid::now_v7(),
        NotificationSeverity::Warning,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        now,
        now,
    )
    .expect("notification");
    OutboundNotificationDelivery::from_notification(&notification, channel, target_revision_id)
        .expect("outbound delivery")
}

#[tokio::test]
async fn signed_webhook_passes_only_canonical_request_and_signing_context_to_connectors() {
    let target_revision_id = ConnectorRevisionId::new();
    let connector = Arc::new(RecordingConnector::accepting());
    let adapter = SignedWebhookNotificationAdapter::new(target_revision_id, connector.clone())
        .expect("signed webhook adapter");
    let debug = format!("{adapter:?}");
    assert!(debug.contains("opaque execution port"));
    assert!(!debug.contains("endpoint"));
    assert!(!debug.contains("secret"));

    let delivery = delivery(
        OutboundNotificationChannel::SignedWebhook,
        target_revision_id,
    );
    let attempt_id = Uuid::now_v7();
    let receipt = adapter
        .deliver(&delivery, attempt_id)
        .await
        .expect("webhook delivery");
    assert_eq!(receipt.delivery_id, delivery.id());
    assert_eq!(receipt.attempt_id, attempt_id);
    assert_eq!(receipt.target_revision_id, target_revision_id);

    let requests = connector.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.attempt_id(), attempt_id);
    assert_eq!(
        request.body(),
        delivery
            .canonical_payload()
            .expect("canonical delivery payload")
    );
    assert_eq!(
        request
            .headers()
            .get("x-a3s-notification-delivery-id")
            .map(String::as_str),
        Some(delivery.id().to_string().as_str())
    );
    let timestamp = request
        .headers()
        .get("x-a3s-notification-timestamp")
        .expect("delivery timestamp");
    assert_eq!(
        request.signing_input(),
        Some(webhook_signing_input(timestamp, delivery.id(), request.body()).as_slice())
    );
    assert!(request.headers().get("authorization").is_none());
    assert!(request
        .headers()
        .get("x-a3s-notification-signature")
        .is_none());
}

#[tokio::test]
async fn slack_compatible_adapter_uses_the_same_connector_execution_port() {
    let target_revision_id = ConnectorRevisionId::new();
    let connector = Arc::new(RecordingConnector::accepting());
    let adapter = SlackCompatibleNotificationAdapter::new(target_revision_id, connector.clone())
        .expect("Slack-compatible adapter");
    let delivery = delivery(
        OutboundNotificationChannel::SlackCompatible,
        target_revision_id,
    );
    adapter
        .deliver(&delivery, Uuid::now_v7())
        .await
        .expect("Slack delivery");

    let requests = connector.requests();
    assert_eq!(requests.len(), 1);
    let payload: serde_json::Value =
        serde_json::from_slice(requests[0].body()).expect("Slack-compatible payload");
    assert_eq!(
        payload,
        serde_json::json!({
            "text": "[warning] Organization role changed\nYour organization role is now member."
        })
    );
    assert!(requests[0].signing_input().is_none());
}

#[tokio::test]
async fn connector_retry_is_classified_without_an_adapter_local_retry_loop() {
    let target_revision_id = ConnectorRevisionId::new();
    let connector = Arc::new(RecordingConnector::failing(
        ConnectorExecutionError::Retryable {
            retry_after: Some(std::time::Duration::from_secs(7)),
        },
    ));
    let adapter = SlackCompatibleNotificationAdapter::new(target_revision_id, connector.clone())
        .expect("Slack-compatible adapter");
    let error = adapter
        .deliver(
            &delivery(
                OutboundNotificationChannel::SlackCompatible,
                target_revision_id,
            ),
            Uuid::now_v7(),
        )
        .await
        .expect_err("retry remains owned by the durable runner");
    assert!(error.is_retryable());
    assert_eq!(error.retry_after(), Some(std::time::Duration::from_secs(7)));
    assert_eq!(connector.requests().len(), 1);
}

#[tokio::test]
async fn connector_receipt_identity_drift_fails_closed() {
    let target_revision_id = ConnectorRevisionId::new();
    let connector = Arc::new(RecordingConnector::drifting(ConnectorRevisionId::new()));
    let adapter = SlackCompatibleNotificationAdapter::new(target_revision_id, connector)
        .expect("Slack-compatible adapter");
    assert_eq!(
        adapter
            .deliver(
                &delivery(
                    OutboundNotificationChannel::SlackCompatible,
                    target_revision_id,
                ),
                Uuid::now_v7(),
            )
            .await,
        Err(OutboundNotificationDeliveryError::Rejected)
    );
}

#[test]
fn nil_connector_revision_is_rejected_before_adapter_construction() {
    let connector = Arc::new(RecordingConnector::accepting());
    assert!(SlackCompatibleNotificationAdapter::new(
        ConnectorRevisionId::from_uuid(Uuid::nil()),
        connector,
    )
    .is_err());
}

#[tokio::test]
async fn nil_attempt_identity_is_rejected_before_connector_execution() {
    let target_revision_id = ConnectorRevisionId::new();
    let connector = Arc::new(RecordingConnector::accepting());
    let adapter = SlackCompatibleNotificationAdapter::new(target_revision_id, connector.clone())
        .expect("Slack-compatible adapter");
    assert_eq!(
        adapter
            .deliver(
                &delivery(
                    OutboundNotificationChannel::SlackCompatible,
                    target_revision_id,
                ),
                Uuid::nil(),
            )
            .await,
        Err(OutboundNotificationDeliveryError::Rejected)
    );
    assert!(connector.requests().is_empty());
}
