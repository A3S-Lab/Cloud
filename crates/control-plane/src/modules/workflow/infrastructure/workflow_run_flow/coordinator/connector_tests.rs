use super::super::FlowWorkflowRunCoordinator;
use crate::modules::connectors::{
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, ConnectorResponseObjectContent,
    ConnectorResponseObjectReference, IConnectorResponseObjectPort, IWorkflowConnectorPort,
    ReadConnectorResponseObject, WorkflowConnectorAttemptRequest, WorkflowConnectorAttemptResult,
    WorkflowConnectorResponseMode,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, IWorkflowRunCoordinator, WorkflowRun, WorkflowRunInput, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowStepFailureClassification, WorkflowStepFailureOutput,
    WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION_V23,
    WORKFLOW_RUN_FLOW_VERSION_V8, WORKFLOW_RUN_FLOW_VERSION_V9,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{
    cancellation_compensating_connector_workflow_run_input, connector_workflow_run_input, digest,
    routed_connector_workflow_run_input, TEST_CONNECTOR_STEP_ID,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowRuntime, RuntimeBuildCompatibility, RuntimeBuildId,
    RuntimeCommand, StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

struct ConnectorTestRuntime(WorkflowRunFlowRuntime);

#[async_trait]
impl FlowRuntime for ConnectorTestRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        self.0.run_workflow(invocation).await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        self.0.run_step(invocation).await
    }
}

struct FakeConnectorResponses;

#[async_trait]
impl IConnectorResponseObjectPort for FakeConnectorResponses {
    async fn read_response_object(
        &self,
        request: &ReadConnectorResponseObject,
    ) -> ApplicationResult<ConnectorResponseObjectContent> {
        ConnectorResponseObjectContent::for_test(
            request.reference.clone(),
            br#"{"accepted":true}"#.to_vec(),
        )
    }
}

pub(super) struct FakeConnectorPort {
    pub(super) requests: Mutex<Vec<WorkflowConnectorAttemptRequest>>,
    pub(super) calls: AtomicUsize,
    mode: FakeConnectorMode,
    retry_not_before: DateTime<Utc>,
}

#[derive(Clone, Copy)]
enum FakeConnectorMode {
    Accepted,
    DeferredOnce,
    Indeterminate,
}

impl FakeConnectorPort {
    fn accepted(now: DateTime<Utc>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            mode: FakeConnectorMode::Accepted,
            retry_not_before: now,
        }
    }

    pub(super) fn deferred_once(retry_not_before: DateTime<Utc>) -> Self {
        Self {
            mode: FakeConnectorMode::DeferredOnce,
            ..Self::accepted(retry_not_before)
        }
    }

    fn indeterminate(now: DateTime<Utc>) -> Self {
        Self {
            mode: FakeConnectorMode::Indeterminate,
            ..Self::accepted(now)
        }
    }
}

#[async_trait]
impl IWorkflowConnectorPort for FakeConnectorPort {
    async fn execute_attempt(
        &self,
        request: &WorkflowConnectorAttemptRequest,
    ) -> ApplicationResult<WorkflowConnectorAttemptResult> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().await.push(request.clone());
        let authority = request
            .connector_attempt_authority()
            .map_err(crate::modules::shared_kernel::application::ApplicationError::Invalid)?;
        if matches!(self.mode, FakeConnectorMode::DeferredOnce) && call == 0 {
            return Ok(WorkflowConnectorAttemptResult::Deferred {
                attempt_id: authority.attempt_id,
                retry_not_before: self.retry_not_before,
            });
        }
        if matches!(self.mode, FakeConnectorMode::Indeterminate) {
            return Ok(WorkflowConnectorAttemptResult::Indeterminate {
                attempt_id: authority.attempt_id,
                dispatch_started_at: self.retry_not_before,
                outcome_deadline_at: self.retry_not_before + Duration::seconds(30),
            });
        }
        let completed_at = canonical_timestamp(Utc::now());
        let evidence = ConnectorExecutionEvidence::restore(
            request.organization_id,
            request.project_id,
            request.environment_id,
            request.connector_profile_id,
            request.connector_revision_id,
            authority.attempt_id,
            authority.request_digest,
            authority.request_body_bytes,
            ConnectorExecutionOutcome::Accepted,
            Some(200),
            Some(Sha256Digest::from_bytes(br#"{"accepted":true}"#)),
            Some(br#"{"accepted":true}"#.len() as u64),
            None,
            completed_at - Duration::milliseconds(1),
            completed_at,
        )
        .map_err(crate::modules::shared_kernel::application::ApplicationError::Invalid)?;
        let response_object = ConnectorResponseObjectReference::from_evidence(&evidence)
            .map_err(crate::modules::shared_kernel::application::ApplicationError::Internal)?;
        Ok(WorkflowConnectorAttemptResult::Completed {
            evidence: Box::new(evidence),
            response_object: Some(response_object),
        })
    }
}

#[tokio::test]
async fn coordinator_resumes_exact_connector_evidence_without_a_child_reference() {
    let (engine, record, now) = fixture().await;
    let port = Arc::new(FakeConnectorPort::accepted(now));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate Connector")
        .expect("completed projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    let requests = port.requests.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].workflow_run_id, record.run.id);
    assert_eq!(requests[0].step_id, TEST_CONNECTOR_STEP_ID);
    assert_eq!(requests[0].step_attempt, 1);
    let attempt_id = requests[0]
        .connector_attempt_authority()
        .expect("Connector attempt authority")
        .attempt_id;
    assert_eq!(
        requests[0].response_mode,
        WorkflowConnectorResponseMode::ImmutableObjectReference
    );
    let step = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Completed);
    assert_eq!(
        step.evidence_references,
        [format!("urn:a3s:cloud:connectors:attempt:{attempt_id}")]
    );
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("snapshot");
    let response_step_sequence = engine
        .history(&record.run.flow_run_id)
        .await
        .expect("history")
        .into_iter()
        .rev()
        .find_map(|event| match event.event {
            FlowEvent::StepCompleted { step_id, .. }
                if step_id == flow_step_id(TEST_CONNECTOR_STEP_ID) =>
            {
                Some(event.sequence)
            }
            _ => None,
        })
        .expect("typed Connector response completion");
    assert_eq!(step.last_flow_sequence, response_step_sequence);
    assert!(snapshot.child_operations.is_empty());
}

#[tokio::test]
async fn coordinator_observes_one_deferred_attempt_after_the_flow_wait() {
    let (engine, record, now) = fixture().await;
    let retry_not_before = now + Duration::seconds(2);
    let port = Arc::new(FakeConnectorPort::deferred_once(retry_not_before));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate deferred Connector")
        .expect("waiting projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    engine
        .resume_due_waits(retry_not_before)
        .await
        .expect("resume Connector observation wait");
    let completed = coordinator
        .reconcile(&waiting, retry_not_before)
        .await
        .expect("coordinate observed Connector")
        .expect("completed projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
    let requests = port.requests.lock().await;
    assert_eq!(
        requests[0].connector_attempt_authority(),
        requests[1].connector_attempt_authority()
    );
    assert_eq!(requests[0].step_attempt, 1);
    assert_eq!(requests[1].step_attempt, 1);
}

#[tokio::test]
async fn coordinator_projects_the_descriptor_bound_connector_failure_route() {
    let (engine, record, now) = routed_failure_fixture().await;
    let port = Arc::new(FakeConnectorPort::indeterminate(now));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate indeterminate Connector")
        .expect("completed failure branch projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    let connector = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector projection");
    assert_eq!(connector.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(connector.selected_handle.as_deref(), Some("error"));
    let attempt_id = port.requests.lock().await[0]
        .connector_attempt_authority()
        .expect("Connector attempt authority")
        .attempt_id;
    assert_eq!(
        connector.evidence_references,
        [format!("urn:a3s:cloud:connectors:attempt:{attempt_id}")]
    );
    assert!(connector.result.is_none());
    assert!(connector
        .error
        .as_deref()
        .is_some_and(|error| error.contains("became indeterminate")));

    let failure_output = completed
        .steps
        .iter()
        .find(|step| step.step_id == "failure_output")
        .expect("failure output projection");
    assert_eq!(
        failure_output.status,
        WorkflowStepProjectionStatus::Completed
    );
    let failure = serde_json::from_value::<WorkflowStepFailureOutput>(
        failure_output.result.clone().expect("typed failure output"),
    )
    .expect("failure output contract");
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::ProviderIndeterminate
    );
    assert_eq!(failure.step_id, TEST_CONNECTOR_STEP_ID);
    assert!(failure.details.is_none());
    assert_eq!(
        completed
            .steps
            .iter()
            .find(|step| step.step_id == "output")
            .expect("success output projection")
            .status,
        WorkflowStepProjectionStatus::Skipped
    );
    assert!(engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot")
        .child_operations
        .is_empty());
}

#[tokio::test]
async fn coordinator_dispatches_cancellation_compensation_before_terminal_projection() {
    let (engine, mut record, now) = fixture_with(
        cancellation_compensating_connector_workflow_run_input()
            .expect("cancellation-compensating Connector WorkflowRun input"),
        WORKFLOW_RUN_FLOW_VERSION_V23,
    )
    .await;
    let port = Arc::new(FakeConnectorPort::accepted(now));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowConnectorPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate completed cancellation source")
        .expect("waiting projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    assert_eq!(port.requests.lock().await[0].step_id, "reserve");

    record = waiting;
    let cancellation_at = canonical_timestamp(record.run.updated_at + Duration::milliseconds(1));
    record
        .run
        .request_cancellation(
            Some("operator requested cancellation".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request cancellation");
    let cancelled = coordinator
        .reconcile(&record, cancellation_at + Duration::milliseconds(1))
        .await
        .expect("coordinate cancellation compensation")
        .expect("cancelled projection");

    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.calls.load(Ordering::SeqCst), 2);
    let requests = port.requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].step_id, "reserve");
    assert_eq!(requests[1].step_id, "release");
    assert!(requests.iter().all(|request| request.step_id != "charge"));
    drop(requests);

    let compensation_hook_id = "workflow-connector-cancellation-compensation:reserve:release:1:1";
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("cancelled snapshot");
    assert_eq!(
        snapshot.hooks[compensation_hook_id].status,
        a3s_flow::HookStatus::Received
    );
    assert_eq!(
        engine
            .history(&record.run.flow_run_id)
            .await
            .expect("cancelled history")
            .iter()
            .filter(|event| matches!(
                &event.event,
                FlowEvent::HookCreated { hook_id, .. } if hook_id == compensation_hook_id
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn connector_projection_rejects_payload_and_creation_history_drift() {
    let (engine, record, now) = fixture().await;
    let port = Arc::new(FakeConnectorPort::accepted(now));
    let coordinator = FlowWorkflowRunCoordinator::with_connectors(
        engine.clone(),
        port as Arc<dyn IWorkflowConnectorPort>,
    );
    coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate Connector")
        .expect("completed projection");
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("snapshot");
    let history = engine
        .history(&record.run.flow_run_id)
        .await
        .expect("history");

    let mut payload_drift = snapshot.clone();
    payload_drift
        .hooks
        .get_mut("workflow-connector:invoke:1:1")
        .and_then(|hook| hook.payload.as_mut())
        .expect("Connector payload")["digest"] = serde_json::json!(digest('f'));
    assert!(super::super::project_workflow_run_record(&record, &payload_drift, &history).is_err());

    let mut response_step_drift = history.clone();
    let step_name = response_step_drift
        .iter_mut()
        .find_map(|event| match &mut event.event {
            a3s_flow::FlowEvent::StepCreated {
                step_id, step_name, ..
            } if step_id == "workflow:invoke" => Some(step_name),
            _ => None,
        })
        .expect("Connector response step creation event");
    *step_name = "workflow_run_local".into();
    assert!(
        super::super::project_workflow_run_record(&record, &snapshot, &response_step_drift)
            .is_err()
    );

    let mut history_drift = history;
    let created = history_drift
        .iter_mut()
        .find_map(|event| match &mut event.event {
            a3s_flow::FlowEvent::HookCreated {
                hook_id, metadata, ..
            } if hook_id == "workflow-connector:invoke:1:1" => Some(metadata),
            _ => None,
        })
        .expect("Connector creation event");
    created["effectiveInputDigest"] = serde_json::json!(digest('e'));
    assert!(super::super::project_workflow_run_record(&record, &snapshot, &history_drift).is_err());
}

pub(super) async fn fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    fixture_with(
        connector_workflow_run_input().expect("Connector WorkflowRun input"),
        WORKFLOW_RUN_FLOW_VERSION_V8,
    )
    .await
}

async fn routed_failure_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    fixture_with(
        routed_connector_workflow_run_input().expect("routed Connector WorkflowRun input"),
        WORKFLOW_RUN_FLOW_VERSION_V9,
    )
    .await
}

async fn fixture_with(
    mut input: WorkflowRunInput,
    flow_version: &str,
) -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + Duration::hours(1);
    input.validate().expect("valid Connector WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-connector-test@1").expect("runtime build");
    let response_runtime =
        WorkflowRunFlowRuntime::with_connector_responses(Arc::new(FakeConnectorResponses));
    let engine = FlowEngine::builder(Arc::new(ConnectorTestRuntime(response_runtime)))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(WORKFLOW_RUN_FLOW_NAME, flow_version, "a3s-cloud", "main")
                .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start WorkflowRun Flow");
    (engine, record, now)
}
