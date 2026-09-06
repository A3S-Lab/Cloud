use super::*;
use crate::modules::automations::domain::AutomationScheduleState;
use crate::modules::automations::infrastructure::InMemoryAutomationScheduleStateRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, Sha256Digest,
};
use a3s_cloud_contracts::{
    AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationInvocationInputV1, AutomationInvocationOriginV1, AutomationRevisionV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const SCHEDULE_DEFINITION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/aut0.1/automation-definition-schedule.acl"
));

fn timestamp(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .expect("timestamp")
        .with_timezone(&Utc)
}

fn revision(id: u128) -> AutomationRevisionV1 {
    let definition = AutomationDefinitionV1::parse_acl(SCHEDULE_DEFINITION).expect("definition");
    AutomationRevisionV1::from_definition(Uuid::from_u128(id), 1, None, definition.spec().clone())
        .expect("revision")
}

fn key(revision: &AutomationRevisionV1) -> AutomationScheduleStateKey {
    let definition = &revision.spec().definition;
    AutomationScheduleStateKey::new(
        OrganizationId::from_uuid(definition.organization_id),
        ProjectId::from_uuid(definition.project_id),
        EnvironmentId::from_uuid(definition.environment_id),
        definition.automation_id,
    )
}

fn authorization(revision: &AutomationRevisionV1) -> AutomationInvocationAuthorizationV1 {
    AutomationInvocationAuthorizationV1 {
        policy_digest: revision
            .spec()
            .definition
            .authorization
            .policy_digest
            .clone(),
        grant_snapshot_digest: format!("sha256:{}", "b".repeat(64)),
        principal_id: Some(Uuid::from_u128(0x018f0000000070008000000000000501)),
    }
}

fn dispatch_request(
    revision: AutomationRevisionV1,
    observed_at: DateTime<Utc>,
    lease_id: u128,
) -> AutomationScheduleDispatchRequest {
    AutomationScheduleDispatchRequest {
        key: key(&revision),
        revision: revision.clone(),
        owner_id: Uuid::from_u128(0x018f0000000070008000000000000502),
        lease_id: Uuid::from_u128(lease_id),
        reserved_at: observed_at,
        lease_expires_at: observed_at + chrono::Duration::minutes(5),
        observed_at,
        limit: 32,
        input: AutomationInvocationInputV1::inline_json(json!({"source": "schedule"}))
            .expect("input"),
        authorization: authorization(&revision),
    }
}

async fn repository(
    revision: &AutomationRevisionV1,
) -> Arc<InMemoryAutomationScheduleStateRepository> {
    let repository = Arc::new(InMemoryAutomationScheduleStateRepository::new());
    let state = AutomationScheduleState::new(
        key(revision),
        revision.spec().revision_id,
        Sha256Digest::parse(revision.digest()).expect("revision digest"),
        timestamp("2026-01-01T00:00:00Z"),
        timestamp("2026-01-01T00:00:00Z"),
    )
    .expect("state");
    repository.create(state).await.expect("create state");
    repository
}

struct RecordingAdmission {
    envelopes: Mutex<Vec<AutomationInvocationEnvelopeV1>>,
    outcome: Mutex<AutomationInvocationAdmissionOutcome>,
    failure: Mutex<Option<ApplicationError>>,
}

impl Default for RecordingAdmission {
    fn default() -> Self {
        Self {
            envelopes: Mutex::new(Vec::new()),
            outcome: Mutex::new(AutomationInvocationAdmissionOutcome::Admitted),
            failure: Mutex::new(None),
        }
    }
}

#[async_trait]
impl IAutomationInvocationAdmission for RecordingAdmission {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome> {
        if let Some(error) = self.failure.lock().expect("failure lock").clone() {
            return Err(error);
        }
        envelope.validate().expect("exact envelope");
        self.envelopes.lock().expect("envelope lock").push(envelope);
        Ok(*self.outcome.lock().expect("outcome lock"))
    }
}

fn service(
    states: Arc<InMemoryAutomationScheduleStateRepository>,
    admission: Arc<RecordingAdmission>,
) -> AutomationScheduleDispatchService {
    AutomationScheduleDispatchService::new(states, admission)
}

#[tokio::test]
async fn admits_exact_latest_occurrence_and_commits_the_cursor() {
    let revision = revision(0x018f0000000070008000000000000510);
    let states = repository(&revision).await;
    let admission = Arc::new(RecordingAdmission {
        outcome: Mutex::new(AutomationInvocationAdmissionOutcome::Admitted),
        ..Default::default()
    });
    let result = service(Arc::clone(&states), Arc::clone(&admission))
        .dispatch(dispatch_request(
            revision.clone(),
            timestamp("2026-01-04T02:00:00Z"),
            0x018f0000000070008000000000000511,
        ))
        .await
        .expect("dispatch");

    assert_eq!(result.admitted, 1);
    assert_eq!(result.already_admitted, 0);
    assert_eq!(result.skipped, 3);
    assert_eq!(
        result.evaluated_through,
        Some(timestamp("2026-01-04T01:00:00Z"))
    );
    let envelopes = admission.envelopes.lock().expect("envelopes");
    assert_eq!(envelopes.len(), 1);
    assert!(matches!(
        envelopes[0].origin,
        AutomationInvocationOriginV1::DueTime { scheduled_at }
            if scheduled_at == timestamp("2026-01-04T01:00:00Z")
    ));
    let state = states
        .find(key(&revision))
        .await
        .expect("find")
        .expect("state");
    assert_eq!(state.cursor_at(), timestamp("2026-01-04T01:00:00Z"));
    assert!(state.lease().is_none());
}

#[tokio::test]
async fn skipped_backlog_advances_without_admission() {
    let revision = revision(0x018f0000000070008000000000000520);
    let states = repository(&revision).await;
    let admission = Arc::new(RecordingAdmission::default());
    let result = service(Arc::clone(&states), Arc::clone(&admission))
        .dispatch(dispatch_request(
            revision.clone(),
            timestamp("2026-01-04T00:30:00Z"),
            0x018f0000000070008000000000000521,
        ))
        .await
        .expect("dispatch");

    assert_eq!(result.admitted, 0);
    assert_eq!(result.skipped, 3);
    assert!(admission.envelopes.lock().expect("envelopes").is_empty());
    let state = states
        .find(key(&revision))
        .await
        .expect("find")
        .expect("state");
    assert_eq!(state.cursor_at(), timestamp("2026-01-03T01:00:00Z"));
    assert!(state.lease().is_none());
}

#[tokio::test]
async fn no_due_work_releases_the_lease_without_cursor_progress() {
    let revision = revision(0x018f0000000070008000000000000530);
    let states = repository(&revision).await;
    let admission = Arc::new(RecordingAdmission::default());
    let result = service(Arc::clone(&states), Arc::clone(&admission))
        .dispatch(dispatch_request(
            revision.clone(),
            timestamp("2026-01-01T00:30:00Z"),
            0x018f0000000070008000000000000531,
        ))
        .await
        .expect("dispatch");

    assert_eq!(
        result,
        AutomationScheduleDispatchResult {
            admitted: 0,
            already_admitted: 0,
            skipped: 0,
            evaluated_through: None,
        }
    );
    let state = states
        .find(key(&revision))
        .await
        .expect("find")
        .expect("state");
    assert_eq!(state.cursor_at(), timestamp("2026-01-01T00:00:00Z"));
    assert!(state.lease().is_none());
}

#[tokio::test]
async fn admission_failure_releases_the_lease_and_preserves_the_cursor() {
    let revision = revision(0x018f0000000070008000000000000540);
    let states = repository(&revision).await;
    let admission = Arc::new(RecordingAdmission {
        failure: Mutex::new(Some(ApplicationError::Unavailable(
            "invocation owner unavailable".into(),
        ))),
        ..Default::default()
    });
    let error = service(Arc::clone(&states), Arc::clone(&admission))
        .dispatch(dispatch_request(
            revision.clone(),
            timestamp("2026-01-04T02:00:00Z"),
            0x018f0000000070008000000000000541,
        ))
        .await
        .expect_err("admission failure");
    assert_eq!(
        error,
        ApplicationError::Unavailable("invocation owner unavailable".into())
    );
    let state = states
        .find(key(&revision))
        .await
        .expect("find")
        .expect("state");
    assert_eq!(state.cursor_at(), timestamp("2026-01-01T00:00:00Z"));
    assert!(state.lease().is_none());
}

#[tokio::test]
async fn replay_admission_is_counted_without_changing_schedule_progress() {
    let revision = revision(0x018f0000000070008000000000000550);
    let states = repository(&revision).await;
    let admission = Arc::new(RecordingAdmission {
        outcome: Mutex::new(AutomationInvocationAdmissionOutcome::AlreadyAdmitted),
        ..Default::default()
    });
    let result = service(Arc::clone(&states), Arc::clone(&admission))
        .dispatch(dispatch_request(
            revision.clone(),
            timestamp("2026-01-04T02:00:00Z"),
            0x018f0000000070008000000000000551,
        ))
        .await
        .expect("replay dispatch");
    assert_eq!(result.admitted, 0);
    assert_eq!(result.already_admitted, 1);
    assert_eq!(admission.envelopes.lock().expect("envelopes").len(), 1);
}

#[tokio::test]
async fn rejects_revision_drift_before_reserving_a_lease() {
    let stored = revision(0x018f0000000070008000000000000560);
    let states = repository(&stored).await;
    let admission = Arc::new(RecordingAdmission::default());
    let mut request = dispatch_request(
        revision(0x018f0000000070008000000000000561),
        timestamp("2026-01-04T02:00:00Z"),
        0x018f0000000070008000000000000562,
    );
    request.key = key(&stored);
    let error = service(Arc::clone(&states), admission)
        .dispatch(request)
        .await
        .expect_err("revision drift");
    assert!(
        matches!(error, ApplicationError::Conflict(message) if message.contains("different revision"))
    );
    let state = states
        .find(key(&stored))
        .await
        .expect("find")
        .expect("state");
    assert!(state.lease().is_none());
}
