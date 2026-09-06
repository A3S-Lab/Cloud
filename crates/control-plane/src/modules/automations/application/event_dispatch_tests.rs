use super::*;
use crate::modules::automations::application::event_fanout::AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES;
use crate::modules::automations::application::event_invocation::AutomationEventInvocationEvaluationService;
use crate::modules::automations::domain::IAutomationEventFilterEvaluator;
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationEventTriggerV1, AutomationInvocationInputV1,
    AutomationTriggerV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const WEBHOOK_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-webhook.acl"
));

fn timestamp(value: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(value, 0).expect("timestamp")
}

fn revision(id: u128) -> AutomationRevisionV1 {
    revision_with_filter(id, None)
}

fn revision_with_filter(id: u128, filter_digest: Option<String>) -> AutomationRevisionV1 {
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
        Uuid::from_u128(0x018f0000000070008000000000000460),
        "plugin.package.updated",
        timestamp(1_767_229_200),
        AutomationInvocationInputV1::inline_json(json!({"source": "dispatch-test"}))
            .expect("input"),
    )
    .expect("event")
}

fn candidate(
    revision: AutomationRevisionV1,
    invocation_id: u128,
) -> AutomationEventInvocationCandidateOwned {
    AutomationEventInvocationCandidateOwned {
        authorization: AutomationInvocationAuthorizationV1 {
            policy_digest: revision
                .spec()
                .definition
                .authorization
                .policy_digest
                .clone(),
            grant_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
            principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000461)),
        },
        revision,
        invocation_id: Uuid::from_u128(invocation_id),
        correlation_id: Uuid::from_u128(invocation_id + 1),
        causation_id: None,
    }
}

struct FixedFilter;

#[async_trait]
impl IAutomationEventFilterEvaluator for FixedFilter {
    async fn matches(
        &self,
        _filter_digest: &crate::modules::shared_kernel::domain::Sha256Digest,
        _event: &AutomationNormalizedEventV1,
    ) -> Result<bool, String> {
        Ok(true)
    }
}

struct RejectingFilter;

#[async_trait]
impl IAutomationEventFilterEvaluator for RejectingFilter {
    async fn matches(
        &self,
        _filter_digest: &crate::modules::shared_kernel::domain::Sha256Digest,
        _event: &AutomationNormalizedEventV1,
    ) -> Result<bool, String> {
        Ok(false)
    }
}

struct FixedCandidates {
    values: Mutex<Vec<AutomationEventInvocationCandidateOwned>>,
}

#[async_trait]
impl IAutomationEventCandidateProvider for FixedCandidates {
    async fn candidates(
        &self,
        _event: &AutomationNormalizedEventV1,
    ) -> ApplicationResult<Vec<AutomationEventInvocationCandidateOwned>> {
        Ok(self.values.lock().expect("candidate lock").clone())
    }
}

struct FailingCandidates {
    calls: AtomicUsize,
}

#[async_trait]
impl IAutomationEventCandidateProvider for FailingCandidates {
    async fn candidates(
        &self,
        _event: &AutomationNormalizedEventV1,
    ) -> ApplicationResult<Vec<AutomationEventInvocationCandidateOwned>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(ApplicationError::Unavailable(
            "candidate authority unavailable".into(),
        ))
    }
}

struct RecordingAdmission {
    calls: AtomicUsize,
    outcomes: Mutex<Vec<AutomationInvocationAdmissionOutcome>>,
    failure: Mutex<Option<ApplicationError>>,
}

#[async_trait]
impl IAutomationInvocationAdmission for RecordingAdmission {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome> {
        assert!(envelope.validate().is_ok());
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.failure.lock().expect("failure lock").clone() {
            return Err(error);
        }
        Ok(self
            .outcomes
            .lock()
            .expect("outcome lock")
            .pop()
            .unwrap_or(AutomationInvocationAdmissionOutcome::Admitted))
    }
}

fn service(
    candidates: Vec<AutomationEventInvocationCandidateOwned>,
    admission: Arc<RecordingAdmission>,
) -> AutomationEventInvocationDispatchService {
    AutomationEventInvocationDispatchService::new(
        Arc::new(FixedCandidates {
            values: Mutex::new(candidates),
        }),
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter)),
        ),
        admission,
    )
}

#[tokio::test]
async fn dispatches_only_fanout_matches_to_idempotent_admission() {
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(vec![AutomationInvocationAdmissionOutcome::AlreadyAdmitted]),
        failure: Mutex::new(None),
    });
    let service = service(
        vec![candidate(
            revision(0x018f0000000070008000000000000462),
            0x018f0000000070008000000000000463,
        )],
        Arc::clone(&admission),
    );
    assert_eq!(
        service
            .dispatch_at(&event(), timestamp(1_767_229_201))
            .await
            .expect("dispatch"),
        1
    );
    assert_eq!(admission.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn empty_candidate_set_is_a_successful_noop() {
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
    });
    let service = service(Vec::new(), Arc::clone(&admission));
    assert_eq!(
        service
            .dispatch_at(&event(), timestamp(1_767_229_201))
            .await
            .expect("dispatch"),
        0
    );
    assert_eq!(admission.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admission_failure_is_returned_for_provider_redelivery() {
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(Some(ApplicationError::Unavailable(
            "admission unavailable".into(),
        ))),
    });
    let service = service(
        vec![candidate(
            revision(0x018f0000000070008000000000000464),
            0x018f0000000070008000000000000465,
        )],
        Arc::clone(&admission),
    );
    let error = service
        .dispatch_at(&event(), timestamp(1_767_229_201))
        .await
        .expect_err("admission failure");
    assert_eq!(
        error,
        ApplicationError::Unavailable("admission unavailable".into())
    );
    assert_eq!(admission.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn candidate_bound_is_rejected_before_admission() {
    let revision = revision(0x018f0000000070008000000000000466);
    let candidates = (0..=AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES)
        .map(|index| {
            candidate(
                revision.clone(),
                0x018f0000000070008000000000000500 + index as u128,
            )
        })
        .collect();
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
    });
    let service = service(candidates, Arc::clone(&admission));
    let error = service
        .dispatch_at(&event(), timestamp(1_767_229_201))
        .await
        .expect_err("candidate bound");
    assert_eq!(
        error,
        ApplicationError::Invalid("Automation event fan-out exceeds 1024 candidates".into())
    );
    assert_eq!(admission.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn non_matching_filter_never_reaches_admission() {
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
    });
    let service = AutomationEventInvocationDispatchService::new(
        Arc::new(FixedCandidates {
            values: Mutex::new(vec![candidate(
                revision_with_filter(
                    0x018f0000000070008000000000000471,
                    Some(format!("sha256:{}", "e".repeat(64))),
                ),
                0x018f0000000070008000000000000472,
            )]),
        }),
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(RejectingFilter)),
        ),
        admission.clone(),
    );
    assert_eq!(
        service
            .dispatch_at(&event(), timestamp(1_767_229_201))
            .await
            .expect("non-match"),
        0
    );
    assert_eq!(admission.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn candidate_authority_failure_is_returned_without_admission() {
    let candidates = Arc::new(FailingCandidates {
        calls: AtomicUsize::new(0),
    });
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
    });
    let service = AutomationEventInvocationDispatchService::new(
        candidates.clone(),
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter)),
        ),
        admission.clone(),
    );
    assert_eq!(
        service
            .dispatch_at(&event(), timestamp(1_767_229_201))
            .await
            .expect_err("candidate authority failure"),
        ApplicationError::Unavailable("candidate authority unavailable".into())
    );
    assert_eq!(candidates.calls.load(Ordering::SeqCst), 1);
    assert_eq!(admission.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn invalid_event_is_rejected_before_candidate_lookup() {
    let candidates = Arc::new(FailingCandidates {
        calls: AtomicUsize::new(0),
    });
    let admission = Arc::new(RecordingAdmission {
        calls: AtomicUsize::new(0),
        outcomes: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
    });
    let service = AutomationEventInvocationDispatchService::new(
        candidates.clone(),
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter)),
        ),
        admission,
    );
    let mut invalid = event();
    invalid.event_digest = "sha256:invalid".into();
    assert!(matches!(
        service
            .dispatch_at(&invalid, timestamp(1_767_229_201))
            .await,
        Err(ApplicationError::Invalid(_))
    ));
    assert_eq!(candidates.calls.load(Ordering::SeqCst), 0);
}

struct ReplayAdmission {
    calls: AtomicUsize,
    fail_on_call: usize,
    stored: Mutex<BTreeMap<Uuid, AutomationInvocationEnvelopeV1>>,
}

#[async_trait]
impl IAutomationInvocationAdmission for ReplayAdmission {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on_call {
            return Err(ApplicationError::Unavailable(
                "temporary admission loss".into(),
            ));
        }
        let mut stored = self.stored.lock().expect("stored invocation lock");
        match stored.get(&envelope.invocation_id) {
            Some(existing) if existing == &envelope => {
                Ok(AutomationInvocationAdmissionOutcome::AlreadyAdmitted)
            }
            Some(_) => Err(ApplicationError::Conflict(
                "invocation replay drifted".into(),
            )),
            None => {
                stored.insert(envelope.invocation_id, envelope);
                Ok(AutomationInvocationAdmissionOutcome::Admitted)
            }
        }
    }
}

#[tokio::test]
async fn partial_admission_redelivery_preserves_the_exact_envelopes() {
    let admission = Arc::new(ReplayAdmission {
        calls: AtomicUsize::new(0),
        fail_on_call: 2,
        stored: Mutex::new(BTreeMap::new()),
    });
    let service = AutomationEventInvocationDispatchService::new(
        Arc::new(FixedCandidates {
            values: Mutex::new(vec![
                candidate(
                    revision(0x018f0000000070008000000000000467),
                    0x018f0000000070008000000000000468,
                ),
                candidate(
                    revision(0x018f0000000070008000000000000469),
                    0x018f0000000070008000000000000470,
                ),
            ]),
        }),
        AutomationEventInvocationFanoutService::new(
            AutomationEventInvocationEvaluationService::new(Arc::new(FixedFilter)),
        ),
        admission.clone(),
    );

    assert_eq!(
        service.handle(event()).await,
        Err("temporary admission loss".into())
    );
    assert_eq!(admission.stored.lock().expect("stored lock").len(), 1);
    service.handle(event()).await.expect("exact redelivery");
    let stored = admission.stored.lock().expect("stored lock");
    assert_eq!(stored.len(), 2);
    assert!(stored
        .values()
        .all(|value| value.requested_at == event().observed_at));
    assert_eq!(admission.calls.load(Ordering::SeqCst), 4);
}
