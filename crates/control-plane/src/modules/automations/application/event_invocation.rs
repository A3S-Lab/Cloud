use crate::modules::automations::domain::{
    AutomationEventInvocationFactory, AutomationEventInvocationRequest,
    IAutomationEventFilterEvaluator,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_contracts::{AutomationInvocationEnvelopeV1, AutomationTriggerV1};
use std::sync::Arc;

/// Application boundary for event-triggered invocation evaluation.
///
/// The domain factory first binds the normalized event to the exact revision.
/// When the revision carries a filter digest, this service asks the
/// Automations-owned evaluator to evaluate that exact digest. A non-match is
/// represented by `None`; this component does not persist, enqueue, or execute
/// an invocation.
#[derive(Clone)]
pub struct AutomationEventInvocationEvaluationService {
    filter_evaluator: Arc<dyn IAutomationEventFilterEvaluator>,
}

impl AutomationEventInvocationEvaluationService {
    pub fn new(filter_evaluator: Arc<dyn IAutomationEventFilterEvaluator>) -> Self {
        Self { filter_evaluator }
    }

    pub async fn evaluate(
        &self,
        request: AutomationEventInvocationRequest<'_>,
    ) -> ApplicationResult<Option<AutomationInvocationEnvelopeV1>> {
        let event = request.event.clone();
        let revision = request.revision;

        // Build and validate the candidate before consulting a filter. Invalid
        // trigger, event, authorization, or revision input must never be
        // hidden by a false filter result.
        let envelope =
            AutomationEventInvocationFactory::build(request).map_err(ApplicationError::Invalid)?;

        let filter_digest = match &revision.spec().definition.trigger {
            AutomationTriggerV1::PluginEvent(trigger)
            | AutomationTriggerV1::SourceEvent(trigger) => trigger
                .filter_digest
                .as_deref()
                .map(|digest| Sha256Digest::parse(digest.to_owned()))
                .transpose()
                .map_err(ApplicationError::Invalid)?,
            _ => None,
        };

        let Some(filter_digest) = filter_digest else {
            return Ok(Some(envelope));
        };

        let matches = self
            .filter_evaluator
            .matches(&filter_digest, &event)
            .await
            .map_err(|_| {
                ApplicationError::Invalid("Automation event filter evaluation failed".into())
            })?;
        Ok(matches.then_some(envelope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationEventTriggerV1, AutomationInvocationAuthorizationV1,
        AutomationInvocationInputV1, AutomationNormalizedEventV1, AutomationRevisionV1,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    const WEBHOOK_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-webhook.acl"
    ));

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp")
    }

    fn revision(filter_digest: Option<String>) -> AutomationRevisionV1 {
        let definition = AutomationDefinitionV1::parse_acl(WEBHOOK_DEFINITION).expect("definition");
        let mut spec = definition.spec().clone();
        let subscription = spec.trigger.subscription().cloned().expect("subscription");
        spec.trigger = AutomationTriggerV1::PluginEvent(AutomationEventTriggerV1 {
            subscription,
            event_key: "plugin.package.updated".into(),
            filter_digest,
        });
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000420),
            1,
            None,
            spec,
        )
        .expect("revision")
    }

    fn request(revision: &AutomationRevisionV1) -> AutomationEventInvocationRequest<'_> {
        AutomationEventInvocationRequest {
            revision,
            invocation_id: Uuid::from_u128(0x018f0000000070008000000000000421),
            event: AutomationNormalizedEventV1::new(
                Uuid::from_u128(0x018f0000000070008000000000000422),
                "plugin.package.updated",
                timestamp(1_767_229_200),
                AutomationInvocationInputV1::inline_json(json!({"source": "filter-test"}))
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
                principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000423)),
            },
            correlation_id: Uuid::from_u128(0x018f0000000070008000000000000424),
            causation_id: None,
        }
    }

    struct FixedFilter {
        matched: bool,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IAutomationEventFilterEvaluator for FixedFilter {
        async fn matches(
            &self,
            filter_digest: &Sha256Digest,
            event: &AutomationNormalizedEventV1,
        ) -> Result<bool, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(filter_digest.as_str(), format!("sha256:{}", "f".repeat(64)));
            assert_eq!(event.event_key, "plugin.package.updated");
            Ok(self.matched)
        }
    }

    struct FailingFilter;

    #[async_trait]
    impl IAutomationEventFilterEvaluator for FailingFilter {
        async fn matches(
            &self,
            _filter_digest: &Sha256Digest,
            _event: &AutomationNormalizedEventV1,
        ) -> Result<bool, String> {
            Err("provider error".into())
        }
    }

    #[tokio::test]
    async fn skips_filter_port_when_revision_has_no_filter() {
        let service = AutomationEventInvocationEvaluationService::new(Arc::new(FailingFilter));
        let revision = revision(None);
        let result = service
            .evaluate(request(&revision))
            .await
            .expect("evaluation");
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn returns_envelope_when_exact_filter_matches() {
        let service = AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter {
            matched: true,
            calls: AtomicUsize::new(0),
        }));
        let revision = revision(Some(format!("sha256:{}", "f".repeat(64))));
        assert!(service
            .evaluate(request(&revision))
            .await
            .expect("evaluation")
            .is_some());
    }

    #[tokio::test]
    async fn returns_none_when_exact_filter_does_not_match() {
        let service = AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter {
            matched: false,
            calls: AtomicUsize::new(0),
        }));
        let revision = revision(Some(format!("sha256:{}", "f".repeat(64))));
        assert!(service
            .evaluate(request(&revision))
            .await
            .expect("evaluation")
            .is_none());
    }

    #[tokio::test]
    async fn redacts_filter_adapter_errors() {
        let service = AutomationEventInvocationEvaluationService::new(Arc::new(FailingFilter));
        let revision = revision(Some(format!("sha256:{}", "f".repeat(64))));
        assert_eq!(
            service.evaluate(request(&revision)).await,
            Err(ApplicationError::Invalid(
                "Automation event filter evaluation failed".into()
            ))
        );
    }
}
