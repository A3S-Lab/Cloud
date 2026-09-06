use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1, AutomationRevisionV1,
    AutomationTriggerV1,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Inputs for constructing one deterministic due-time invocation envelope.
///
/// The exact revision remains owned by its existing revision authority. This
/// factory only turns an already-selected schedule occurrence into the common
/// invocation handoff; it does not admit, persist, enqueue, or execute it.
pub struct AutomationScheduleInvocationRequest<'a> {
    pub revision: &'a AutomationRevisionV1,
    pub invocation_id: Uuid,
    pub scheduled_at: DateTime<Utc>,
    pub requested_at: DateTime<Utc>,
    pub input: AutomationInvocationInputV1,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AutomationScheduleInvocationFactory;

impl AutomationScheduleInvocationFactory {
    pub fn build(
        request: AutomationScheduleInvocationRequest<'_>,
    ) -> Result<AutomationInvocationEnvelopeV1, String> {
        request.revision.validate()?;
        let definition = &request.revision.spec().definition;
        if !matches!(definition.trigger, AutomationTriggerV1::Schedule(_)) {
            return Err("Automation due-time invocation requires a schedule revision".into());
        }
        if request.scheduled_at > request.requested_at {
            return Err(
                "Automation due-time invocation cannot be requested before its scheduled time"
                    .into(),
            );
        }
        let origin = AutomationInvocationOriginV1::DueTime {
            scheduled_at: request.scheduled_at,
        };
        let deduplication_key = definition.policy.deduplication.render_key(
            definition.automation_id,
            request.revision.spec().revision_id,
            &origin,
            None,
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
            subscription: None,
            deduplication_key,
            input: request.input,
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
        AutomationDefinitionV1, AutomationInvocationOriginV1, AutomationRevisionV1,
    };
    use chrono::DateTime;
    use serde_json::json;

    const SCHEDULE_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));
    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn revision() -> AutomationRevisionV1 {
        let definition =
            AutomationDefinitionV1::parse_acl(SCHEDULE_DEFINITION).expect("definition");
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000401),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision")
    }

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp")
    }

    fn request(revision: &AutomationRevisionV1) -> AutomationScheduleInvocationRequest<'_> {
        AutomationScheduleInvocationRequest {
            revision,
            invocation_id: Uuid::from_u128(0x018f0000000070008000000000000402),
            scheduled_at: timestamp(1_767_229_200),
            requested_at: timestamp(1_767_229_201),
            input: AutomationInvocationInputV1::inline_json(json!({"source": "schedule"}))
                .expect("input"),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest: format!("sha256:{}", "c".repeat(64)),
                principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000403)),
            },
            correlation_id: Uuid::from_u128(0x018f0000000070008000000000000404),
            causation_id: None,
        }
    }

    #[test]
    fn builds_exact_due_time_envelope_with_policy_derived_deduplication() {
        let revision = revision();
        let envelope =
            AutomationScheduleInvocationFactory::build(request(&revision)).expect("envelope");
        assert!(matches!(
            envelope.origin,
            AutomationInvocationOriginV1::DueTime { .. }
        ));
        assert!(envelope.deduplication_key.contains("scheduled/"));
        envelope
            .validate_for_revision(&revision)
            .expect("exact revision binding");
    }

    #[test]
    fn rejects_an_event_revision_as_a_due_time_source() {
        let definition =
            AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("event definition");
        let event_revision = AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000406),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("event revision");
        assert!(AutomationScheduleInvocationFactory::build(request(&event_revision)).is_err());
    }

    #[test]
    fn derives_the_same_deduplication_key_for_a_replayed_occurrence() {
        let revision = revision();
        let first =
            AutomationScheduleInvocationFactory::build(request(&revision)).expect("first envelope");
        let mut replay_request = request(&revision);
        replay_request.invocation_id = Uuid::from_u128(0x018f0000000070008000000000000407);
        let replay =
            AutomationScheduleInvocationFactory::build(replay_request).expect("replay envelope");
        assert_eq!(first.deduplication_key, replay.deduplication_key);
        assert_ne!(first.invocation_id, replay.invocation_id);
    }

    #[test]
    fn rejects_authorization_policy_drift_and_future_due_time() {
        let revision = revision();
        let mut policy_drift = request(&revision);
        policy_drift.authorization.policy_digest = format!("sha256:{}", "d".repeat(64));
        assert!(AutomationScheduleInvocationFactory::build(policy_drift).is_err());

        let mut future = request(&revision);
        future.scheduled_at = timestamp(1_767_229_202);
        assert!(AutomationScheduleInvocationFactory::build(future).is_err());
    }
}
