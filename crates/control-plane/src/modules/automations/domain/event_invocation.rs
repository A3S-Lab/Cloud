use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationOriginV1, AutomationNormalizedEventV1, AutomationRevisionV1,
    AutomationTriggerV1,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Inputs for constructing one deterministic invocation from an already
/// normalized plugin or Source event.
///
/// Event normalization and filtering remain owned by the subscription/source
/// authority. This factory only binds the normalized identity to one exact
/// Automation revision; it does not consume, persist, enqueue, or execute it.
pub struct AutomationEventInvocationRequest<'a> {
    pub revision: &'a AutomationRevisionV1,
    pub invocation_id: Uuid,
    pub event: AutomationNormalizedEventV1,
    pub requested_at: DateTime<Utc>,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationEventInvocationFactory;

impl AutomationEventInvocationFactory {
    pub fn build(
        request: AutomationEventInvocationRequest<'_>,
    ) -> Result<AutomationInvocationEnvelopeV1, String> {
        request.revision.validate()?;
        request.event.validate()?;
        let definition = &request.revision.spec().definition;
        let subscription = match &definition.trigger {
            AutomationTriggerV1::PluginEvent(trigger)
            | AutomationTriggerV1::SourceEvent(trigger) => {
                if trigger.event_key != request.event.event_key {
                    return Err(
                        "Automation normalized event key does not match its trigger revision"
                            .into(),
                    );
                }
                trigger.subscription.clone()
            }
            _ => {
                return Err(
                    "Automation event invocation requires a plugin or Source event revision".into(),
                )
            }
        };
        if request.event.observed_at > request.requested_at {
            return Err(
                "Automation event invocation cannot be requested before event observation".into(),
            );
        }
        let origin = AutomationInvocationOriginV1::Event {
            event_id: request.event.event_id,
            event_key: request.event.event_key.clone(),
            event_digest: request.event.event_digest.clone(),
            observed_at: request.event.observed_at,
        };
        let deduplication_key = definition.policy.deduplication.render_key(
            definition.automation_id,
            request.revision.spec().revision_id,
            &origin,
            Some(subscription.subscription_id),
        )?;
        let envelope = AutomationInvocationEnvelopeV1 {
            schema: AutomationInvocationEnvelopeV1::SCHEMA.into(),
            invocation_id: request.invocation_id,
            automation_id: definition.automation_id,
            automation_revision_id: request.revision.spec().revision_id,
            automation_revision_digest: request.revision.digest().into(),
            organization_id: definition.organization_id,
            project_id: definition.project_id,
            environment_id: definition.environment_id,
            target: definition.target.clone(),
            origin,
            subscription: Some(subscription),
            deduplication_key,
            input: request.event.input,
            authorization: request.authorization,
            requested_at: request.requested_at,
            correlation_id: request.correlation_id,
            causation_id: request.causation_id,
        };
        envelope.validate_for_revision(request.revision)?;
        Ok(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationEventTriggerV1, AutomationInvocationInputV1,
        AutomationInvocationOriginV1, AutomationNormalizedEventV1, AutomationRevisionV1,
        AutomationTriggerV1,
    };
    use chrono::DateTime;
    use serde_json::json;

    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp")
    }

    fn revision(source_event: bool) -> AutomationRevisionV1 {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
        let mut spec = definition.spec().clone();
        let subscription = match &spec.trigger {
            AutomationTriggerV1::Webhook(trigger) => trigger.subscription.clone(),
            _ => panic!("webhook fixture trigger"),
        };
        spec.trigger = if source_event {
            AutomationTriggerV1::SourceEvent(AutomationEventTriggerV1 {
                subscription,
                event_key: "source.pull-request-change.committed".into(),
                filter_digest: Some(format!("sha256:{}", "f".repeat(64))),
            })
        } else {
            AutomationTriggerV1::PluginEvent(AutomationEventTriggerV1 {
                subscription,
                event_key: "plugin.package.updated".into(),
                filter_digest: None,
            })
        };
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(if source_event {
                0x018f0000000070008000000000000411
            } else {
                0x018f0000000070008000000000000410
            }),
            1,
            None,
            spec,
        )
        .expect("revision")
    }

    fn request(revision: &AutomationRevisionV1) -> AutomationEventInvocationRequest<'_> {
        let event_key = match &revision.spec().definition.trigger {
            AutomationTriggerV1::PluginEvent(trigger)
            | AutomationTriggerV1::SourceEvent(trigger) => trigger.event_key.clone(),
            _ => panic!("event fixture trigger"),
        };
        AutomationEventInvocationRequest {
            revision,
            invocation_id: Uuid::from_u128(0x018f0000000070008000000000000412),
            event: AutomationNormalizedEventV1::new(
                Uuid::from_u128(0x018f0000000070008000000000000413),
                event_key,
                timestamp(1_767_229_200),
                AutomationInvocationInputV1::inline_json(json!({
                    "source": "normalized-event"
                }))
                .expect("input"),
            )
            .expect("event"),
            requested_at: timestamp(1_767_229_201),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
                principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000414)),
            },
            correlation_id: Uuid::from_u128(0x018f0000000070008000000000000415),
            causation_id: Some(Uuid::from_u128(0x018f0000000070008000000000000416)),
        }
    }

    #[test]
    fn builds_exact_plugin_event_envelope_with_subscription_deduplication() {
        let revision = revision(false);
        let envelope =
            AutomationEventInvocationFactory::build(request(&revision)).expect("envelope");
        assert!(matches!(
            envelope.origin,
            AutomationInvocationOriginV1::Event { .. }
        ));
        assert!(envelope.deduplication_key.contains("subscription/"));
        assert_eq!(
            envelope.subscription.as_ref(),
            revision.spec().definition.trigger.subscription()
        );
        envelope
            .validate_for_revision(&revision)
            .expect("exact event revision binding");
    }

    #[test]
    fn accepts_source_event_revision_with_its_exact_event_key() {
        let revision = revision(true);
        let envelope =
            AutomationEventInvocationFactory::build(request(&revision)).expect("envelope");
        assert_eq!(
            envelope.origin.event_id(),
            Some(Uuid::from_u128(0x018f0000000070008000000000000413,))
        );
    }

    #[test]
    fn rejects_event_key_drift_non_event_revision_and_observation_reordering() {
        let revision = revision(false);
        let mut wrong_key = request(&revision);
        wrong_key.event = AutomationNormalizedEventV1::new(
            wrong_key.event.event_id,
            "plugin.package.deleted",
            wrong_key.event.observed_at,
            wrong_key.event.input,
        )
        .expect("valid wrong-key event");
        assert!(AutomationEventInvocationFactory::build(wrong_key).is_err());

        let definition =
            AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("webhook definition");
        let webhook_revision = AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000417),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("webhook revision");
        let mut non_event_request = request(&revision);
        non_event_request.revision = &webhook_revision;
        assert!(AutomationEventInvocationFactory::build(non_event_request).is_err());

        let mut reordered = request(&revision);
        reordered.requested_at = timestamp(1_767_229_199);
        assert!(AutomationEventInvocationFactory::build(reordered).is_err());
    }

    #[test]
    fn rejects_event_identity_drift_during_envelope_validation() {
        let revision = revision(false);
        let mut value = request(&revision);
        value.event.event_digest = format!("sha256:{}", "c".repeat(64));
        assert!(AutomationEventInvocationFactory::build(value).is_err());
        let envelope =
            AutomationEventInvocationFactory::build(request(&revision)).expect("envelope");
        let mut drifted = envelope;
        drifted.origin = AutomationInvocationOriginV1::Event {
            event_id: Uuid::from_u128(0x018f0000000070008000000000000418),
            event_key: "plugin.package.updated".into(),
            event_digest: format!("sha256:{}", "c".repeat(64)),
            observed_at: timestamp(1_767_229_200),
        };
        assert!(drifted.validate_for_revision(&revision).is_err());
    }
}
