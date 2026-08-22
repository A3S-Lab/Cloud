use super::*;
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, ProjectId,
};

fn notification(
    organization_id: OrganizationId,
    recipient: PrincipalId,
    occurred_at: DateTime<Utc>,
) -> Notification {
    Notification::project(
        organization_id,
        recipient,
        Uuid::now_v7(),
        "workload.health.changed".into(),
        1,
        Uuid::now_v7(),
        1,
        Uuid::now_v7(),
        NotificationSeverity::Critical,
        "Workload unhealthy".into(),
        "The workload health check failed.".into(),
        super::super::NotificationScope::Organization,
        occurred_at,
        occurred_at,
    )
    .expect("notification")
}

fn definition() -> OutboundNotificationSubscriptionDefinition {
    OutboundNotificationSubscriptionDefinition::from_spec(OutboundNotificationSubscriptionSpec {
        channel: OutboundNotificationChannel::SignedWebhook,
        minimum_severity: NotificationSeverity::Warning,
        target: OutboundNotificationConnectorTarget::new(
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
        )
        .expect("target")
        .into(),
    })
    .expect("definition")
}

#[test]
fn subscription_acl_is_canonical_exact_and_smtp_closed() {
    let definition = definition();
    let spec = definition.spec();
    let target = spec.target.connector().expect("Connector target");
    let expected_acl = format!(
        concat!(
            "notification_outbound_subscription {{\n",
            "  channel = \"signed_webhook\"\n",
            "  connector_environment_id = \"{}\"\n",
            "  connector_profile_id = \"{}\"\n",
            "  connector_project_id = \"{}\"\n",
            "  connector_revision_id = \"{}\"\n",
            "  minimum_severity = \"warning\"\n",
            "  schema = \"cloud.notification.outbound-subscription.v1\"\n",
            "}}\n"
        ),
        target.environment_id, target.profile_id, target.project_id, target.revision_id,
    );
    assert_eq!(definition.canonical_acl(), expected_acl);
    assert_eq!(
        OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl())
            .expect("reparse"),
        definition
    );
    assert!(definition.canonical_acl().ends_with('\n'));
    assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
        &definition
            .canonical_acl()
            .replace("  channel", "    channel")
    )
    .is_err());
    let mut smtp = definition.spec();
    smtp.channel = OutboundNotificationChannel::Smtp;
    assert!(OutboundNotificationSubscriptionDefinition::from_spec(smtp).is_err());
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA
    );
    assert_eq!(
        definition.maximum_provider_attempts(),
        MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS
    );
    assert!(!definition
        .canonical_acl()
        .contains("maximum_provider_attempts"));
    assert_eq!(definition.suppress_before(), None);
}

#[test]
fn version_two_acl_pins_a_bounded_provider_attempt_budget() {
    let spec = definition().spec();
    let definition =
        OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(spec, 3)
            .expect("version two definition");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
    );
    assert_eq!(definition.schema_version(), 2);
    assert_eq!(definition.maximum_provider_attempts(), 3);
    assert_eq!(definition.suppress_before(), None);
    assert!(definition
        .canonical_acl()
        .contains("maximum_provider_attempts = 3"));
    assert_eq!(
        OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl()),
        Ok(definition.clone())
    );
    assert!(
        OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(spec, 0)
            .is_err()
    );
    assert!(
        OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(spec, 9)
            .is_err()
    );
    assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
        &definition
            .canonical_acl()
            .replace("  maximum_provider_attempts = 3\n", "")
    )
    .is_err());
    assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
        &definition.canonical_acl().replace(
            "  maximum_provider_attempts = 3",
            "  maximum_provider_attempts = 2.5"
        )
    )
    .is_err());
}

#[test]
fn checked_in_version_two_contract_is_canonical() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/c0.3/outbound-notification-subscription-v2.acl"
    ));
    let definition =
        OutboundNotificationSubscriptionDefinition::parse_acl(source).expect("contract");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V2
    );
    assert_eq!(definition.maximum_provider_attempts(), 3);
}

#[test]
fn version_three_acl_suppresses_strictly_by_bounded_source_event_time() {
    let organization_id = OrganizationId::new();
    let recipient = PrincipalId::new();
    let created_at = canonical_timestamp(Utc::now());
    let suppress_before = created_at + Duration::days(1);
    let definition = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
        definition().spec(),
        3,
        suppress_before,
    )
    .expect("version three definition");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
    );
    assert_eq!(definition.schema_version(), 3);
    assert_eq!(definition.delivery_schema_version(), 2);
    assert_eq!(definition.maximum_provider_attempts(), 3);
    assert_eq!(definition.suppress_before(), Some(suppress_before));
    assert!(definition.canonical_acl().contains(&format!(
        "suppress_before = \"{}\"",
        suppress_before.to_rfc3339_opts(SecondsFormat::Micros, true)
    )));
    assert_eq!(
        OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl()),
        Ok(definition.clone())
    );

    let subscription = OutboundNotificationSubscription::create(
        organization_id,
        NotificationSubscriptionId::new(),
        recipient,
        definition.clone(),
        recipient,
        created_at,
    )
    .expect("bounded suppressed subscription");
    assert!(!subscription.matches(&notification(
        organization_id,
        recipient,
        suppress_before - Duration::microseconds(1),
    )));
    let boundary = notification(organization_id, recipient, suppress_before);
    assert!(subscription.matches(&boundary));
    let delivery = definition
        .delivery_for(&boundary)
        .expect("eligible delivery");
    assert_eq!(delivery.schema_version(), 2);
    assert_eq!(delivery.maximum_provider_attempts(), 3);

    for invalid_cutoff in [
        created_at,
        created_at + Duration::days(30) + Duration::microseconds(1),
    ] {
        let invalid = OutboundNotificationSubscriptionDefinition::from_spec_with_suppression(
            definition.spec(),
            3,
            invalid_cutoff,
        )
        .expect("definition is independent from subscription creation time");
        assert!(OutboundNotificationSubscription::create(
            organization_id,
            NotificationSubscriptionId::new(),
            recipient,
            invalid,
            recipient,
            created_at,
        )
        .is_err());
    }
    assert!(OutboundNotificationSubscriptionDefinition::parse_acl(
        &definition
            .canonical_acl()
            .replace("  suppress_before = ", "  unknown_suppression = ")
    )
    .is_err());
}

#[test]
fn checked_in_version_three_contract_is_canonical() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/c0.3/outbound-notification-subscription-v3.acl"
    ));
    let definition =
        OutboundNotificationSubscriptionDefinition::parse_acl(source).expect("contract");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V3
    );
    assert_eq!(definition.maximum_provider_attempts(), 3);
    assert_eq!(
        definition
            .suppress_before()
            .map(|value| value.to_rfc3339_opts(SecondsFormat::Micros, true)),
        Some("2026-09-01T00:00:00.000000Z".into())
    );
}

#[test]
fn version_four_smtp_acl_uses_only_an_opaque_verified_contact_target() {
    let contact_id = RecipientContactId::new();
    let definition = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Critical,
        3,
        None,
    )
    .expect("version four SMTP definition");
    let expected_acl = format!(
        concat!(
            "notification_outbound_subscription {{\n",
            "  channel = \"smtp\"\n",
            "  maximum_provider_attempts = 3\n",
            "  minimum_severity = \"critical\"\n",
            "  recipient_contact_id = \"{}\"\n",
            "  schema = \"cloud.notification.outbound-subscription.v4\"\n",
            "}}\n"
        ),
        contact_id,
    );
    assert_eq!(definition.canonical_acl(), expected_acl);
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4
    );
    assert_eq!(definition.schema_version(), 4);
    assert_eq!(definition.delivery_schema_version(), 3);
    assert_eq!(definition.spec().channel, OutboundNotificationChannel::Smtp);
    assert_eq!(
        definition.spec().target.recipient_contact_id(),
        Some(contact_id)
    );
    assert_eq!(
        OutboundNotificationSubscriptionDefinition::parse_acl(definition.canonical_acl()),
        Ok(definition.clone())
    );
    for forbidden in [
        "connector_project_id",
        "connector_environment_id",
        "connector_profile_id",
        "connector_revision_id",
        "address",
        "mailbox",
        "credential",
    ] {
        assert!(!definition.canonical_acl().contains(forbidden));
    }

    let cutoff = canonical_timestamp(Utc::now() + Duration::days(1));
    let suppressed = OutboundNotificationSubscriptionDefinition::from_smtp_spec(
        contact_id,
        NotificationSeverity::Warning,
        1,
        Some(cutoff),
    )
    .expect("suppressed SMTP definition");
    assert_eq!(suppressed.suppress_before(), Some(cutoff));
    assert_eq!(
        OutboundNotificationSubscriptionDefinition::parse_acl(suppressed.canonical_acl()),
        Ok(suppressed)
    );
    let noncanonical_cutoff = DateTime::parse_from_rfc3339("2026-09-01T00:00:00.000000001Z")
        .expect("timestamp")
        .with_timezone(&Utc);
    assert_eq!(
        validate_versioned_policy(
            OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4,
            1,
            Some(noncanonical_cutoff),
        ),
        Err("outbound notification subscription v4 requires a canonical suppression cutoff".into())
    );
}

#[test]
fn checked_in_version_four_contract_is_canonical() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/c0.3/outbound-notification-subscription-v4.acl"
    ));
    let definition =
        OutboundNotificationSubscriptionDefinition::parse_acl(source).expect("contract");
    assert_eq!(
        definition.definition_schema(),
        OUTBOUND_NOTIFICATION_SUBSCRIPTION_SCHEMA_V4
    );
    assert_eq!(definition.maximum_provider_attempts(), 3);
    assert_eq!(
        definition
            .spec()
            .target
            .recipient_contact_id()
            .map(|value| value.to_string()),
        Some("55555555-5555-7555-8555-555555555555".into())
    );
}

#[test]
fn subscription_is_personal_immutable_and_only_revocable_once() {
    let recipient = PrincipalId::new();
    let now = canonical_timestamp(Utc::now());
    let subscription = OutboundNotificationSubscription::create(
        OrganizationId::new(),
        NotificationSubscriptionId::new(),
        recipient,
        definition(),
        recipient,
        now,
    )
    .expect("subscription");
    assert!(subscription.is_active());
    let revoked = subscription
        .revoke(1, recipient, now)
        .expect("revoked subscription");
    assert!(!revoked.is_active());
    assert!(revoked.revoke(2, recipient, now).is_err());
    assert!(OutboundNotificationSubscription::create(
        OrganizationId::new(),
        NotificationSubscriptionId::new(),
        PrincipalId::new(),
        definition(),
        recipient,
        now,
    )
    .is_err());
}
