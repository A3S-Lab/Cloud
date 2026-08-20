use super::super::FlowWorkflowRunCoordinator;
use crate::modules::connectors::{
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, IWorkflowConnectorPort,
    WorkflowConnectorAttemptRequest, WorkflowConnectorAttemptResult,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, Sha256Digest};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowRun, WorkflowRunRecord, WorkflowRunStatus,
    WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION_V5,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{
    connector_workflow_run_input, digest, TEST_CONNECTOR_STEP_ID,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Copy)]
struct ConnectorTestRuntime;

#[async_trait]
impl FlowRuntime for ConnectorTestRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        WorkflowRunFlowRuntime.run_workflow(invocation).await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        WorkflowRunFlowRuntime.run_step(invocation).await
    }
}

struct FakeConnectorPort {
    requests: Mutex<Vec<WorkflowConnectorAttemptRequest>>,
    calls: AtomicUsize,
    defer_first: bool,
    retry_not_before: DateTime<Utc>,
}

impl FakeConnectorPort {
    fn accepted(now: DateTime<Utc>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
            defer_first: false,
            retry_not_before: now,
        }
    }

    fn deferred_once(retry_not_before: DateTime<Utc>) -> Self {
        Self {
            defer_first: true,
            ..Self::accepted(retry_not_before)
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
        if self.defer_first && call == 0 {
            return Ok(WorkflowConnectorAttemptResult::Deferred {
                attempt_id: authority.attempt_id,
                retry_not_before: self.retry_not_before,
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
        Ok(WorkflowConnectorAttemptResult::Completed { evidence })
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
    let step = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_CONNECTOR_STEP_ID)
        .expect("Connector projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Completed);
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("snapshot");
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

async fn fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let mut input = connector_workflow_run_input().expect("Connector WorkflowRun input");
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + Duration::hours(1);
    input.validate().expect("valid Connector WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-connector-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(ConnectorTestRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                WORKFLOW_RUN_FLOW_NAME,
                WORKFLOW_RUN_FLOW_VERSION_V5,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start WorkflowRun Flow");
    (engine, record, now)
}
