use super::*;
use crate::modules::notifications::domain::{
    Notification, NotificationScope, NotificationSeverity, OutboundNotificationConnectorTarget,
};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, PrincipalId, ProjectId,
};
use chrono::Utc;

fn delivery(channel: OutboundNotificationChannel) -> OutboundNotificationDelivery {
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
    OutboundNotificationDelivery::from_notification(
        &notification,
        channel,
        OutboundNotificationConnectorTarget::new(
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
        )
        .expect("target"),
    )
    .expect("outbound delivery")
}

#[test]
fn signed_webhook_builds_only_a_stable_fenced_connector_request() {
    let adapter = SignedWebhookNotificationAdapter::new();
    let delivery = delivery(OutboundNotificationChannel::SignedWebhook);
    let attempt_id = Uuid::now_v7();
    let request = adapter
        .build_request(&delivery, attempt_id)
        .expect("webhook request");
    let replay = adapter
        .build_request(&delivery, attempt_id)
        .expect("stable replay");

    assert_eq!(request, replay);
    assert_eq!(
        request.connector_revision_id(),
        delivery.target_revision_id()
    );
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
    let occurred_at = request
        .headers()
        .get("x-a3s-notification-occurred-at")
        .expect("stable occurrence timestamp");
    assert_eq!(
        request.signing_input(),
        Some(webhook_signing_input(occurred_at, delivery.id(), request.body()).as_slice())
    );
    assert!(request.headers().get("authorization").is_none());
    assert!(request
        .headers()
        .get("x-a3s-notification-signature")
        .is_none());
    let debug = format!("{adapter:?} {request:?}");
    assert!(!debug.contains(delivery.body()));
    assert!(!debug.contains("endpoint"));
    assert!(!debug.contains("secret"));
}

#[test]
fn slack_compatible_builds_the_same_provider_neutral_request_contract() {
    let adapter = SlackCompatibleNotificationAdapter::new();
    let delivery = delivery(OutboundNotificationChannel::SlackCompatible);
    let request = adapter
        .build_request(&delivery, Uuid::now_v7())
        .expect("Slack-compatible request");
    let payload: serde_json::Value =
        serde_json::from_slice(request.body()).expect("Slack-compatible payload");
    assert_eq!(
        payload,
        serde_json::json!({
            "text": "[warning] Organization role changed\nYour organization role is now member."
        })
    );
    assert!(request.signing_input().is_none());
}

#[test]
fn channel_drift_and_nil_attempt_fail_before_connector_execution() {
    let webhook = delivery(OutboundNotificationChannel::SignedWebhook);
    assert_eq!(
        SlackCompatibleNotificationAdapter::new().build_request(&webhook, Uuid::now_v7()),
        Err(OutboundNotificationRequestError::Rejected)
    );

    let slack = delivery(OutboundNotificationChannel::SlackCompatible);
    assert_eq!(
        SlackCompatibleNotificationAdapter::new().build_request(&slack, Uuid::nil()),
        Err(OutboundNotificationRequestError::Rejected)
    );
}

#[test]
fn unsupported_smtp_has_no_http_adapter_fallback() {
    let smtp = delivery(OutboundNotificationChannel::Smtp);
    assert_eq!(
        SignedWebhookNotificationAdapter::new().build_request(&smtp, Uuid::now_v7()),
        Err(OutboundNotificationRequestError::Rejected)
    );
    assert_eq!(
        SlackCompatibleNotificationAdapter::new().build_request(&smtp, Uuid::now_v7()),
        Err(OutboundNotificationRequestError::Rejected)
    );
}
