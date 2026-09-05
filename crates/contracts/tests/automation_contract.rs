use a3s_cloud_contracts::{
    AutomationApplicationTargetV1, AutomationAuditActionV1, AutomationAuditRecordV1,
    AutomationAuthorizationPolicyV1, AutomationConcurrencyModeV1, AutomationConcurrencyPolicyV1,
    AutomationDeduplicationPolicyV1, AutomationDeduplicationScopeV1, AutomationDefinitionSpecV1,
    AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1, AutomationMisfireModeV1,
    AutomationMisfirePolicyV1, AutomationOutboxMessageV1, AutomationRevisionV1,
    AutomationScheduleTriggerV1, AutomationSubscriptionReferenceV1, AutomationTargetV1,
    AutomationTriggerPolicyV1, AutomationWebhookTriggerV1, AutomationWorkflowTargetV1,
    AUTOMATION_DEFINITION_SCHEMA_V1, AUTOMATION_INVOCATION_SCHEMA_V1,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

const SCHEDULE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-schedule.acl"
));
const WEBHOOK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-webhook.acl"
));

#[test]
fn checked_in_definitions_are_canonical_and_restore_by_digest() {
    for source in [SCHEDULE, WEBHOOK] {
        let definition = AutomationDefinitionV1::parse_acl(source).expect("canonical definition");
        assert_eq!(definition.canonical_acl(), &source.replace("\r\n", "\n"));
        assert!(definition.digest().starts_with("sha256:"));
        assert_eq!(
            AutomationDefinitionV1::restore(definition.canonical_acl(), definition.digest())
                .expect("restored definition"),
            definition
        );
        definition.validate().expect("definition invariants");
    }
}

#[test]
fn definitions_cover_only_exact_target_union_and_closed_trigger_union() {
    let schedule = AutomationDefinitionV1::parse_acl(SCHEDULE).expect("schedule");
    assert!(matches!(
        schedule.spec().target,
        AutomationTargetV1::WorkflowRevision(_)
    ));
    assert!(matches!(
        schedule.spec().trigger,
        a3s_cloud_contracts::AutomationTriggerV1::Schedule(_)
    ));

    let webhook = AutomationDefinitionV1::parse_acl(WEBHOOK).expect("webhook");
    assert!(matches!(
        webhook.spec().target,
        AutomationTargetV1::ApplicationRelease(_)
    ));
    assert_eq!(
        webhook
            .spec()
            .trigger
            .subscription()
            .expect("webhook subscription")
            .subscription_id,
        id(0x407)
    );

    let unknown = SCHEDULE.replace(
        "  workflow_revision {",
        "  latest {\n    selector = \"latest\"\n  }\n  workflow_revision {",
    );
    assert!(AutomationDefinitionV1::parse_acl(&unknown).is_err());

    let duplicate_target = SCHEDULE.replacen(
        "  schedule {",
        "  workflow_revision {\n    revision_digest = \"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"\n    workflow_definition_id = \"018f0000-0000-7000-8000-000000000305\"\n    workflow_revision_id = \"018f0000-0000-7000-8000-000000000306\"\n  }\n  schedule {",
        1,
    );
    assert!(AutomationDefinitionV1::parse_acl(&duplicate_target).is_err());
}

#[test]
fn revision_lineage_is_contiguous_and_never_allows_a_noop_successor() {
    let first = AutomationRevisionV1::from_definition(id(0x501), 1, None, schedule_spec())
        .expect("first revision");
    first.validate().expect("first revision invariants");

    let mut successor_spec = schedule_spec();
    successor_spec.name = "daily-report-v2".into();
    let successor =
        AutomationRevisionV1::from_definition(id(0x502), 2, Some(&first), successor_spec)
            .expect("successor revision");
    successor
        .validate_successor_of(&first)
        .expect("contiguous successor");

    let noop = AutomationRevisionV1::from_definition(id(0x503), 2, Some(&first), schedule_spec())
        .expect("construct noop");
    assert!(noop.validate_successor_of(&first).is_err());

    let gap = AutomationRevisionV1::from_definition(
        id(0x504),
        4,
        Some(&first),
        successor.spec().definition.clone(),
    )
    .expect("construct gap");
    assert!(gap.validate_successor_of(&first).is_err());

    let mut unsorted = schedule_spec();
    unsorted.authorization.required_grants.reverse();
    let normalized = AutomationRevisionV1::from_definition(id(0x505), 1, None, unsorted)
        .expect("normalize grants in revision");
    assert_eq!(
        normalized.spec().definition.authorization.required_grants,
        vec!["automation:invoke", "workflow:run"]
    );
}

#[test]
fn invocation_envelope_binds_policy_derived_key_and_exact_revision() {
    let revision = AutomationRevisionV1::from_definition(id(0x601), 1, None, schedule_spec())
        .expect("revision");
    let requested_at = timestamp("2026-09-05T00:00:00.000Z");
    let origin = AutomationInvocationOriginV1::DueTime {
        scheduled_at: requested_at,
    };
    let key = revision
        .spec()
        .definition
        .policy
        .deduplication
        .render_key(
            revision.spec().definition.automation_id,
            revision.spec().revision_id,
            &origin,
            None,
        )
        .expect("deduplication key");
    let envelope = AutomationInvocationEnvelopeV1 {
        schema: AUTOMATION_INVOCATION_SCHEMA_V1.into(),
        invocation_id: id(0x602),
        automation_id: revision.spec().definition.automation_id,
        automation_revision_id: revision.spec().revision_id,
        automation_revision_digest: revision.digest().into(),
        organization_id: revision.spec().definition.organization_id,
        project_id: revision.spec().definition.project_id,
        environment_id: revision.spec().definition.environment_id,
        target: revision.spec().definition.target.clone(),
        origin,
        subscription: None,
        deduplication_key: key,
        input: AutomationInvocationInputV1::inline_json(json!({"release": "stable"}))
            .expect("input"),
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: digest('b'),
            principal_id: Some(id(0x603)),
        },
        requested_at,
        correlation_id: id(0x604),
        causation_id: None,
    };
    envelope
        .validate_for_revision(&revision)
        .expect("exact invocation envelope");
    assert!(envelope
        .digest()
        .expect("envelope digest")
        .starts_with("sha256:"));

    let mut forged = envelope;
    forged.deduplication_key.push_str("-forged");
    assert!(forged.validate_for_revision(&revision).is_err());
}

#[test]
fn event_invocations_require_subscription_and_matching_normalized_event() {
    let revision = AutomationRevisionV1::from_definition(id(0x701), 1, None, webhook_spec())
        .expect("webhook revision");
    let origin = AutomationInvocationOriginV1::Event {
        event_id: id(0x702),
        event_key: "source.release.published".into(),
        event_digest: digest('e'),
        observed_at: timestamp("2026-09-05T00:01:00.000Z"),
    };
    let subscription = revision
        .spec()
        .definition
        .trigger
        .subscription()
        .expect("webhook subscription");
    let key = revision
        .spec()
        .definition
        .policy
        .deduplication
        .render_key(
            revision.spec().definition.automation_id,
            revision.spec().revision_id,
            &origin,
            Some(subscription.subscription_id),
        )
        .expect("event deduplication key");
    let envelope = AutomationInvocationEnvelopeV1 {
        schema: AUTOMATION_INVOCATION_SCHEMA_V1.into(),
        invocation_id: id(0x703),
        automation_id: revision.spec().definition.automation_id,
        automation_revision_id: revision.spec().revision_id,
        automation_revision_digest: revision.digest().into(),
        organization_id: revision.spec().definition.organization_id,
        project_id: revision.spec().definition.project_id,
        environment_id: revision.spec().definition.environment_id,
        target: revision.spec().definition.target.clone(),
        origin,
        subscription: Some(subscription.clone()),
        deduplication_key: key,
        input: AutomationInvocationInputV1::inline_json(json!({"tag": "v1"})).expect("input"),
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: digest('f'),
            principal_id: None,
        },
        requested_at: timestamp("2026-09-05T00:01:01.000Z"),
        correlation_id: id(0x704),
        causation_id: Some(id(0x705)),
    };
    envelope
        .validate_for_revision(&revision)
        .expect("event envelope");

    let mut wrong_subscription = envelope.clone();
    wrong_subscription.subscription = Some(AutomationSubscriptionReferenceV1 {
        subscription_id: id(0x706),
        revision_digest: digest('1'),
    });
    assert!(wrong_subscription.validate_for_revision(&revision).is_err());

    let mut wrong_event = envelope;
    wrong_event.origin = AutomationInvocationOriginV1::Event {
        event_id: id(0x707),
        event_key: "source.release.deleted".into(),
        event_digest: digest('2'),
        observed_at: timestamp("2026-09-05T00:01:00.000Z"),
    };
    assert!(wrong_event.validate_for_revision(&revision).is_err());
}

#[test]
fn audit_and_outbox_facts_are_redacted_and_bind_the_same_invocation() {
    let revision = AutomationRevisionV1::from_definition(id(0x801), 1, None, schedule_spec())
        .expect("revision");
    let origin = AutomationInvocationOriginV1::DueTime {
        scheduled_at: timestamp("2026-09-05T00:00:00.000Z"),
    };
    let key = revision
        .spec()
        .definition
        .policy
        .deduplication
        .render_key(
            revision.spec().definition.automation_id,
            revision.spec().revision_id,
            &origin,
            None,
        )
        .expect("key");
    let envelope = AutomationInvocationEnvelopeV1 {
        schema: AUTOMATION_INVOCATION_SCHEMA_V1.into(),
        invocation_id: id(0x802),
        automation_id: revision.spec().definition.automation_id,
        automation_revision_id: revision.spec().revision_id,
        automation_revision_digest: revision.digest().into(),
        organization_id: revision.spec().definition.organization_id,
        project_id: revision.spec().definition.project_id,
        environment_id: revision.spec().definition.environment_id,
        target: revision.spec().definition.target.clone(),
        origin,
        subscription: None,
        deduplication_key: key,
        input: AutomationInvocationInputV1::inline_json(json!({"ok": true})).expect("input"),
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: digest('a'),
            principal_id: Some(id(0x803)),
        },
        requested_at: timestamp("2026-09-05T00:00:01.000Z"),
        correlation_id: id(0x804),
        causation_id: None,
    };
    envelope.validate_for_revision(&revision).expect("envelope");
    let audit = AutomationAuditRecordV1::for_invocation(
        &envelope,
        AutomationAuditActionV1::InvocationAdmitted,
        id(0x805),
        timestamp("2026-09-05T00:00:02.000Z"),
    )
    .expect("audit");
    let outbox = AutomationOutboxMessageV1::for_invocation(
        &envelope,
        id(0x806),
        None,
        timestamp("2026-09-05T00:00:02.000Z"),
    )
    .expect("outbox");
    audit.validate().expect("audit invariants");
    outbox.validate().expect("outbox invariants");
    assert_eq!(audit.invocation_id, outbox.invocation_id);
    assert_eq!(outbox.event_key(), "automation.invocation.admitted");
    let serialized = serde_json::to_string(&outbox).expect("serialize");
    assert!(!serialized.contains("ok"));
}

#[test]
fn strict_wire_shapes_and_policy_bounds_fail_closed() {
    let mut wire = serde_json::to_value(
        AutomationInvocationInputV1::inline_json(json!({"a": 1})).expect("input"),
    )
    .expect("wire");
    wire.as_object_mut()
        .expect("object")
        .insert("retryCount".into(), json!(3));
    assert!(serde_json::from_value::<AutomationInvocationInputV1>(wire).is_err());

    let mut invalid = schedule_spec();
    invalid.policy.concurrency.maximum = 0;
    assert!(AutomationDefinitionV1::from_spec(invalid).is_err());

    let mut invalid = webhook_spec();
    invalid.policy.deduplication.key_template = "automation/{automation_id}".into();
    assert!(AutomationDefinitionV1::from_spec(invalid).is_err());
}

#[test]
fn automation_contracts_are_send_and_sync_and_have_acl_only_fixtures() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AutomationDefinitionV1>();
    assert_send_sync::<AutomationRevisionV1>();
    assert_send_sync::<AutomationInvocationEnvelopeV1>();
    assert_send_sync::<AutomationAuditRecordV1>();
    assert_send_sync::<AutomationOutboxMessageV1>();

    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contracts/aut0.1");
    let files = std::fs::read_dir(directory)
        .expect("read AUT0.1 fixtures")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("README.md"))
        .collect::<Vec<_>>();
    assert!(!files.is_empty());
    assert!(files
        .iter()
        .all(|path| path.extension().and_then(|value| value.to_str()) == Some("acl")));
}

fn schedule_spec() -> AutomationDefinitionSpecV1 {
    AutomationDefinitionSpecV1 {
        schema: AUTOMATION_DEFINITION_SCHEMA_V1.into(),
        automation_id: id(0x901),
        organization_id: id(0x902),
        project_id: id(0x903),
        environment_id: id(0x904),
        name: "daily-report".into(),
        trigger: a3s_cloud_contracts::AutomationTriggerV1::Schedule(AutomationScheduleTriggerV1 {
            expression: "0 0 9 * * * *".into(),
            timezone: "Asia/Shanghai".into(),
        }),
        target: AutomationTargetV1::WorkflowRevision(AutomationWorkflowTargetV1 {
            workflow_definition_id: id(0x905),
            workflow_revision_id: id(0x906),
            revision_digest: digest('b'),
        }),
        policy: AutomationTriggerPolicyV1 {
            deduplication: AutomationDeduplicationPolicyV1 {
                scope: AutomationDeduplicationScopeV1::Automation,
                key_template:
                    "automation/{automation_id}/revision/{revision_id}/scheduled/{scheduled_at}"
                        .into(),
                window_ms: 86_400_000,
            },
            concurrency: AutomationConcurrencyPolicyV1 {
                maximum: 8,
                mode: AutomationConcurrencyModeV1::Queue,
            },
            misfire: AutomationMisfirePolicyV1 {
                mode: AutomationMisfireModeV1::FireOnce,
                grace_ms: 3_600_000,
            },
        },
        authorization: AutomationAuthorizationPolicyV1 {
            policy_digest: digest('a'),
            required_grants: vec!["workflow:run".into(), "automation:invoke".into()],
        },
    }
}

fn webhook_spec() -> AutomationDefinitionSpecV1 {
    AutomationDefinitionSpecV1 {
        schema: AUTOMATION_DEFINITION_SCHEMA_V1.into(),
        automation_id: id(0x911),
        organization_id: id(0x912),
        project_id: id(0x913),
        environment_id: id(0x914),
        name: "release-webhook".into(),
        trigger: a3s_cloud_contracts::AutomationTriggerV1::Webhook(AutomationWebhookTriggerV1 {
            subscription: AutomationSubscriptionReferenceV1 {
                subscription_id: id(0x915),
                revision_digest: digest('f'),
            },
            request_schema_digest: digest('e'),
        }),
        target: AutomationTargetV1::ApplicationRelease(AutomationApplicationTargetV1 {
            application_id: id(0x916),
            application_release_id: id(0x917),
            release_digest: digest('d'),
        }),
        policy: AutomationTriggerPolicyV1 {
            deduplication: AutomationDeduplicationPolicyV1 {
                scope: AutomationDeduplicationScopeV1::Subscription,
                key_template:
                    "automation/{automation_id}/revision/{revision_id}/subscription/{subscription_id}/event/{event_id}"
                        .into(),
                window_ms: 604_800_000,
            },
            concurrency: AutomationConcurrencyPolicyV1 {
                maximum: 2,
                mode: AutomationConcurrencyModeV1::Drop,
            },
            misfire: AutomationMisfirePolicyV1 {
                mode: AutomationMisfireModeV1::Skip,
                grace_ms: 0,
            },
        },
        authorization: AutomationAuthorizationPolicyV1 {
            policy_digest: digest('c'),
            required_grants: vec!["application:invoke".into(), "automation:invoke".into()],
        },
    }
}

fn id(value: u16) -> Uuid {
    Uuid::from_u128(0x018f_0000_0000_7000_8000_0000_0000_0000_u128 + u128::from(value))
}

fn digest(character: char) -> String {
    format!("sha256:{}", character.to_string().repeat(64))
}

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}
