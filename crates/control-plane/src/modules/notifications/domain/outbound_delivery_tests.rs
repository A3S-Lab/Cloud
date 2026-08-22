use super::*;
use crate::modules::notifications::{
    Notification, OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionSpec,
    OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2,
};

fn notification() -> Notification {
    let now = Utc::now();
    Notification::project(
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
    .expect("notification")
}

fn target(revision_id: ConnectorRevisionId) -> OutboundNotificationConnectorTarget {
    OutboundNotificationConnectorTarget::new(
        ProjectId::new(),
        EnvironmentId::new(),
        ConnectorProfileId::new(),
        revision_id,
    )
    .expect("target")
}

#[test]
fn delivery_identity_is_stable_per_notification_channel_and_target_revision() {
    let notification = notification();
    let target_revision_id = ConnectorRevisionId::new();
    let first = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SignedWebhook,
        target(target_revision_id),
    )
    .expect("delivery");
    let replay = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SignedWebhook,
        first.target(),
    )
    .expect("delivery replay");
    let another_channel = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SlackCompatible,
        first.target(),
    )
    .expect("Slack-compatible delivery");
    assert_eq!(first, replay);
    assert_ne!(first.id, another_channel.id);

    let payload: serde_json::Value =
        serde_json::from_slice(&first.canonical_payload().expect("canonical payload"))
            .expect("payload JSON");
    assert_eq!(
        payload["schema"],
        serde_json::json!(OUTBOUND_NOTIFICATION_SCHEMA)
    );
    assert_eq!(payload["deliveryId"], serde_json::json!(first.id));
    assert_eq!(
        OutboundNotificationDelivery::from_payload(&payload),
        Ok(first.clone())
    );
    assert!(payload.get("readAt").is_none());
    assert!(payload.get("endpoint").is_none());
    assert!(payload.get("credential").is_none());
    let event = first.requested_event().expect("requested event");
    assert_eq!(event.event_id, first.requested_event_id());
    assert_eq!(event.event_key, OUTBOUND_NOTIFICATION_EVENT_KEY);
    assert_eq!(event.aggregate_id, first.id());
    assert_eq!(event.causation_id, Some(first.source_event_id()));
    assert_eq!(event.payload, payload);
}

#[test]
fn nil_or_tampered_target_identity_fails_closed() {
    let notification = notification();
    assert!(OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SignedWebhook,
        OutboundNotificationConnectorTarget {
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            profile_id: ConnectorProfileId::new(),
            revision_id: ConnectorRevisionId::from_uuid(Uuid::nil()),
        },
    )
    .is_err());
    let mut delivery = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SignedWebhook,
        target(ConnectorRevisionId::new()),
    )
    .expect("delivery");
    let OutboundNotificationTarget::Connector(target) = &mut delivery.target else {
        panic!("Connector target");
    };
    target.revision_id = ConnectorRevisionId::new();
    assert!(delivery.validate().is_err());
}

#[test]
fn version_two_delivery_pins_budget_while_version_one_bytes_remain_unchanged() {
    let notification = notification();
    let target = target(ConnectorRevisionId::new());
    let version_one = OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SignedWebhook,
        target,
    )
    .expect("version one delivery");
    let definition =
        OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
            OutboundNotificationSubscriptionSpec {
                channel: OutboundNotificationChannel::SignedWebhook,
                minimum_severity: NotificationSeverity::Warning,
                target: target.into(),
            },
            2,
        )
        .expect("version two subscription");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
    );
    let version_two = definition
        .delivery_for(&notification)
        .expect("version two delivery");

    assert_eq!(version_one.schema_version(), 1);
    assert_eq!(
        version_one.maximum_provider_attempts(),
        MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
    );
    let version_one_target = version_one.connector_target().expect("Connector target");
    let expected_version_one_payload = serde_json::json!({
        "schema": OUTBOUND_NOTIFICATION_SCHEMA,
        "deliveryId": version_one.id(),
        "channel": version_one.channel(),
        "projectId": version_one_target.project_id,
        "environmentId": version_one_target.environment_id,
        "targetProfileId": version_one_target.profile_id,
        "targetRevisionId": version_one_target.revision_id,
        "organizationId": version_one.organization_id(),
        "notificationId": version_one.notification_id(),
        "recipientPrincipalId": version_one.recipient_principal_id(),
        "sourceEventId": version_one.source_event_id(),
        "sourceEventKey": version_one.source_event_key(),
        "correlationId": version_one.correlation_id(),
        "severity": version_one.severity(),
        "title": version_one.title(),
        "body": version_one.body(),
        "scope": version_one.scope(),
        "occurredAt": version_one.occurred_at(),
    });
    assert_eq!(
        version_one.canonical_payload().expect("version one bytes"),
        canonical_json_bounded(
            &expected_version_one_payload,
            MAXIMUM_OUTBOUND_PAYLOAD_BYTES,
            "outbound notification payload",
        )
        .expect("historic version one bytes")
    );
    assert_eq!(
        version_one.requested_event_id(),
        Uuid::new_v5(&version_one.id(), b"notification-delivery-requested:v1")
    );
    assert!(!version_one
        .canonical_payload_value()
        .expect("version one payload")
        .as_object()
        .expect("object")
        .contains_key("maximumProviderAttempts"));
    assert_eq!(version_two.id(), version_one.id());
    assert_ne!(
        version_two.requested_event_id(),
        version_one.requested_event_id()
    );
    assert_eq!(version_two.schema(), OUTBOUND_NOTIFICATION_SCHEMA_V2);
    assert_eq!(version_two.schema_version(), 2);
    assert_eq!(version_two.maximum_provider_attempts(), 2);
    let payload = version_two
        .canonical_payload_value()
        .expect("version two payload");
    assert_eq!(payload["schema"], OUTBOUND_NOTIFICATION_SCHEMA_V2);
    assert_eq!(payload["maximumProviderAttempts"], 2);
    assert_eq!(
        OutboundNotificationDelivery::from_payload(&payload),
        Ok(version_two.clone())
    );
    assert_eq!(
        version_two
            .requested_event()
            .expect("version two event")
            .schema_version,
        2
    );

    let mut invalid = payload;
    invalid["maximumProviderAttempts"] = serde_json::json!(0);
    assert!(OutboundNotificationDelivery::from_payload(&invalid).is_err());
}

#[test]
fn version_three_smtp_delivery_contains_only_contact_identity_and_content() {
    let notification = notification();
    let contact_id = RecipientContactId::new();
    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Warning,
        3,
        None,
    )
    .expect("SMTP definition");
    let delivery = definition
        .delivery_for(&notification)
        .expect("SMTP delivery");
    let payload = delivery
        .canonical_payload_value()
        .expect("SMTP delivery payload");

    assert_eq!(delivery.schema_version(), 3);
    assert_eq!(delivery.schema(), OUTBOUND_NOTIFICATION_SCHEMA_V3);
    assert_eq!(delivery.channel(), OutboundNotificationChannel::Smtp);
    assert_eq!(delivery.connector_target(), None);
    assert_eq!(delivery.recipient_contact_id(), Some(contact_id));
    assert_eq!(payload["schema"], OUTBOUND_NOTIFICATION_SCHEMA_V3);
    assert_eq!(payload["recipientContactId"], contact_id.to_string());
    assert_eq!(payload["maximumProviderAttempts"], 3);
    for forbidden in [
        "projectId",
        "environmentId",
        "targetProfileId",
        "targetRevisionId",
        "address",
        "mailbox",
        "addressDigest",
        "credential",
        "providerResponse",
    ] {
        assert!(payload.get(forbidden).is_none());
    }
    assert_eq!(
        OutboundNotificationDelivery::from_payload(&payload),
        Ok(delivery.clone())
    );
    assert_eq!(
        delivery.requested_event_id(),
        Uuid::new_v5(&delivery.id(), b"notification-delivery-requested:v3")
    );

    let mut leaked = payload;
    leaked["address"] = serde_json::json!("private@example.test");
    assert!(OutboundNotificationDelivery::from_payload(&leaked).is_err());
}
