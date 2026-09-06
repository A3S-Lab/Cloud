use crate::modules::automations::application::event_invocation::AutomationEventInvocationEvaluationService;
use crate::modules::automations::domain::AutomationEventInvocationRequest;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationNormalizedEventV1, AutomationRevisionV1,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Maximum number of exact revision candidates evaluated for one normalized
/// event. Fan-out is intentionally bounded before any filter adapter call.
pub const AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES: usize = 1_024;

/// Per-revision immutable identity needed to evaluate one normalized event.
/// Candidate discovery and subscription ownership remain outside Automations;
/// this value carries only the already-authorized exact revision handoff.
#[derive(Debug, Clone)]
pub struct AutomationEventInvocationCandidate<'a> {
    pub revision: &'a AutomationRevisionV1,
    pub invocation_id: Uuid,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

/// Deterministic, bounded fan-out over already selected Automation revisions.
///
/// This service only returns invocation envelopes. It does not discover or
/// persist subscriptions, publish events, enqueue work, or execute targets.
#[derive(Clone)]
pub struct AutomationEventInvocationFanoutService {
    evaluation: AutomationEventInvocationEvaluationService,
}

impl AutomationEventInvocationFanoutService {
    pub fn new(evaluation: AutomationEventInvocationEvaluationService) -> Self {
        Self { evaluation }
    }

    pub async fn evaluate(
        &self,
        event: &AutomationNormalizedEventV1,
        requested_at: DateTime<Utc>,
        mut candidates: Vec<AutomationEventInvocationCandidate<'_>>,
    ) -> ApplicationResult<Vec<AutomationInvocationEnvelopeV1>> {
        if candidates.len() > AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES {
            return Err(ApplicationError::Invalid(format!(
                "Automation event fan-out exceeds {} candidates",
                AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES
            )));
        }
        event.validate().map_err(ApplicationError::Invalid)?;
        for candidate in &candidates {
            candidate
                .revision
                .validate()
                .map_err(ApplicationError::Invalid)?;
        }

        candidates.sort_unstable_by(|left, right| {
            left.revision
                .digest()
                .cmp(right.revision.digest())
                .then_with(|| left.invocation_id.cmp(&right.invocation_id))
        });

        let mut identities = BTreeSet::new();
        let mut envelopes = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let identity = (
                candidate.revision.digest().to_owned(),
                candidate.invocation_id,
            );
            if !identities.insert(identity) {
                return Err(ApplicationError::Invalid(
                    "Automation event fan-out contains a duplicate candidate".into(),
                ));
            }
            let envelope = self
                .evaluation
                .evaluate(AutomationEventInvocationRequest {
                    revision: candidate.revision,
                    invocation_id: candidate.invocation_id,
                    event: event.clone(),
                    requested_at,
                    authorization: candidate.authorization,
                    correlation_id: candidate.correlation_id,
                    causation_id: candidate.causation_id,
                })
                .await?;
            if let Some(envelope) = envelope {
                envelopes.push(envelope);
            }
        }
        Ok(envelopes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::domain::IAutomationEventFilterEvaluator;
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationEventTriggerV1, AutomationInvocationInputV1,
        AutomationTriggerV1,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::Arc;

    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp")
    }

    fn revision(id: u128, filter_digest: Option<String>) -> AutomationRevisionV1 {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
        let mut spec = definition.spec().clone();
        let subscription = spec.trigger.subscription().cloned().expect("subscription");
        spec.trigger = AutomationTriggerV1::PluginEvent(AutomationEventTriggerV1 {
            subscription,
            event_key: "plugin.package.updated".into(),
            filter_digest,
        });
        AutomationRevisionV1::from_definition(Uuid::from_u128(id), 1, None, spec).expect("revision")
    }

    fn event() -> AutomationNormalizedEventV1 {
        AutomationNormalizedEventV1::new(
            Uuid::from_u128(0x018f0000000070008000000000000430),
            "plugin.package.updated",
            timestamp(1_767_229_200),
            AutomationInvocationInputV1::inline_json(json!({"source": "fanout-test"}))
                .expect("input"),
        )
        .expect("event")
    }

    fn candidate(
        revision: &AutomationRevisionV1,
        invocation_id: u128,
    ) -> AutomationEventInvocationCandidate<'_> {
        AutomationEventInvocationCandidate {
            revision,
            invocation_id: Uuid::from_u128(invocation_id),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
                principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000431)),
            },
            correlation_id: Uuid::from_u128(invocation_id + 1),
            causation_id: None,
        }
    }

    struct FixedFilter(bool);

    #[async_trait]
    impl IAutomationEventFilterEvaluator for FixedFilter {
        async fn matches(
            &self,
            _filter_digest: &crate::modules::shared_kernel::domain::Sha256Digest,
            _event: &AutomationNormalizedEventV1,
        ) -> Result<bool, String> {
            Ok(self.0)
        }
    }

    fn service(matches: bool) -> AutomationEventInvocationFanoutService {
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter(matches))),
        )
    }

    #[tokio::test]
    async fn sorts_exact_candidates_and_returns_only_filter_matches() {
        let first = revision(0x018f0000000070008000000000000432, None);
        let second = revision(0x018f0000000070008000000000000433, None);
        let result = service(true)
            .evaluate(
                &event(),
                timestamp(1_767_229_201),
                vec![
                    candidate(&second, 0x018f0000000070008000000000000435),
                    candidate(&first, 0x018f0000000070008000000000000434),
                ],
            )
            .await
            .expect("fan-out");
        assert_eq!(result.len(), 2);
        assert!(result[0].automation_revision_digest <= result[1].automation_revision_digest);
    }

    #[tokio::test]
    async fn suppresses_non_matching_candidates_without_side_effects() {
        let revision = revision(
            0x018f0000000070008000000000000436,
            Some(format!("sha256:{}", "f".repeat(64))),
        );
        assert!(service(false)
            .evaluate(
                &event(),
                timestamp(1_767_229_201),
                vec![candidate(&revision, 0x018f0000000070008000000000000437)]
            )
            .await
            .expect("fan-out")
            .is_empty());
    }

    #[tokio::test]
    async fn rejects_duplicate_and_unbounded_candidate_sets() {
        let revision = revision(0x018f0000000070008000000000000438, None);
        let duplicate = candidate(&revision, 0x018f0000000070008000000000000439);
        let duplicate_error = service(true)
            .evaluate(
                &event(),
                timestamp(1_767_229_201),
                vec![duplicate.clone(), duplicate],
            )
            .await
            .expect_err("duplicate candidate");
        assert_eq!(
            duplicate_error,
            ApplicationError::Invalid(
                "Automation event fan-out contains a duplicate candidate".into()
            )
        );

        let candidates = std::iter::repeat_n(
            candidate(&revision, 0x018f000000007000800000000000043a),
            AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES + 1,
        )
        .collect();
        let bound_error = service(true)
            .evaluate(&event(), timestamp(1_767_229_201), candidates)
            .await
            .expect_err("fan-out bound");
        assert_eq!(
            bound_error,
            ApplicationError::Invalid("Automation event fan-out exceeds 1024 candidates".into())
        );
    }

    #[tokio::test]
    async fn rejects_a_request_timestamp_before_event_observation() {
        let revision = revision(0x018f000000007000800000000000043b, None);
        let error = service(true)
            .evaluate(
                &event(),
                timestamp(1_767_229_199),
                vec![candidate(&revision, 0x018f000000007000800000000000043c)],
            )
            .await
            .expect_err("request timestamp ordering");
        assert_eq!(
            error,
            ApplicationError::Invalid(
                "Automation event invocation cannot be requested before event observation".into()
            )
        );
    }
}
