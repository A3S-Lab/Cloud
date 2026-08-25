use super::*;
use crate::modules::executions::{
    ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionStatus, ExecutionTemplate,
    WorkflowExecutionBinding,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, sha256_digest, ExecutionId, PrincipalId, Sha256Digest, WorkflowRunId,
};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowCompositeRegionPolicy, WorkflowExecutionStepOutput,
    WorkflowIterationFailureMode, WorkflowIterationRegionPolicy, WorkflowRun, WorkflowRunFlowState,
    WorkflowStepFailureClassification, WorkflowStepFailureOutput, WorkflowStepProjectionStatus,
    WORKFLOW_PLAN_MAX_BYTES, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_FLOW_VERSION_V4,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{
    application_frame_answer_workflow_run_inputs, composite_workflow_run_input,
    execution_workflow_run_input, routed_execution_workflow_run_input, workflow_run_input,
    TEST_EXECUTION_STEP_ID,
};
use crate::modules::workflow::{
    IWorkflowCompositeExecutionPort, WorkflowCompositeExecutionRequest,
};
use a3s_flow::{
    FlowEvent, FlowRuntime, RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Barrier, Mutex};

#[derive(Debug, Clone, Copy)]
struct TestFlowRuntime;

#[async_trait]
impl FlowRuntime for TestFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        if invocation.spec.name == EXECUTION_WORKFLOW_NAME
            && invocation.spec.version == EXECUTION_WORKFLOW_VERSION
        {
            return Ok(invocation
                .context()
                .complete(serde_json::json!({"test": true})));
        }
        if invocation.spec.name == WORKFLOW_RUN_FLOW_NAME
            && invocation
                .input
                .get("goal_input")
                .and_then(|input| input.get("ticketId"))
                .and_then(serde_json::Value::as_str)
                == Some("FAIL")
        {
            return Ok(invocation.context().fail("test composite child failed"));
        }
        WorkflowRunFlowRuntime::default()
            .run_workflow(invocation)
            .await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        WorkflowRunFlowRuntime::default().run_step(invocation).await
    }
}

struct FakeWorkflowExecutionPort {
    engine: FlowEngine,
    execution: Mutex<Option<Execution>>,
    creates: AtomicUsize,
    terminal_on_start: bool,
}

impl FakeWorkflowExecutionPort {
    fn queued(engine: FlowEngine) -> Self {
        Self {
            engine,
            execution: Mutex::new(None),
            creates: AtomicUsize::new(0),
            terminal_on_start: false,
        }
    }

    fn terminal(engine: FlowEngine) -> Self {
        Self {
            terminal_on_start: true,
            ..Self::queued(engine)
        }
    }

    async fn finish(&self, outcome: ExecutionOutcome, at: DateTime<Utc>) {
        let mut stored = self.execution.lock().await;
        let execution = stored.as_mut().expect("Workflow child Execution");
        if execution.status.is_terminal() {
            return;
        }
        execution
            .begin_cleanup(outcome, at)
            .expect("begin child cleanup");
        execution
            .complete_cleanup(at)
            .expect("complete child cleanup");
    }

    async fn status(&self) -> ExecutionStatus {
        self.execution
            .lock()
            .await
            .as_ref()
            .expect("Workflow child Execution")
            .status
    }

    fn create_count(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IWorkflowExecutionPort for FakeWorkflowExecutionPort {
    async fn start_or_adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let mut stored = self.execution.lock().await;
        if let Some(execution) = stored.as_ref() {
            return Ok(execution.clone());
        }
        let mut execution = Execution::create_with_workflow(
            request.organization_id,
            request.project_id,
            request.environment_id,
            ExecutionId::new(),
            execution_template(request.input.clone()),
            Some(WorkflowExecutionBinding::from(request)),
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.engine
            .start_with_id(
                execution.operation_id.to_string(),
                WorkflowSpec::rust_embedded(
                    EXECUTION_WORKFLOW_NAME,
                    EXECUTION_WORKFLOW_VERSION,
                    "a3s-cloud",
                    "main",
                )
                .with_runtime_build(
                    RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1")
                        .map_err(|error| ApplicationError::Internal(error.to_string()))?,
                ),
                serde_json::json!({
                    "organizationId": execution.organization_id,
                    "executionId": execution.id,
                }),
            )
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        if self.terminal_on_start {
            execution
                .begin_cleanup(
                    ExecutionOutcome::Succeeded { exit_code: 0 },
                    request.requested_at + chrono::Duration::milliseconds(1),
                )
                .map_err(ApplicationError::Invalid)?;
            execution
                .complete_cleanup(request.requested_at + chrono::Duration::milliseconds(1))
                .map_err(ApplicationError::Invalid)?;
        }
        *stored = Some(execution.clone());
        Ok(execution)
    }

    async fn adopt(
        &self,
        request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        Ok(self.execution.lock().await.clone())
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowExecutionRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        let mut stored = self.execution.lock().await;
        let Some(execution) = stored.as_mut() else {
            return Ok(None);
        };
        if !execution.status.is_terminal()
            && !matches!(
                execution.status,
                ExecutionStatus::Cancelling | ExecutionStatus::CleanupPending
            )
        {
            execution
                .request_cancellation(requested_at)
                .map_err(ApplicationError::Conflict)?;
        }
        Ok(Some(execution.clone()))
    }
}

struct RejectingWorkflowExecutionPort;

#[async_trait]
impl IWorkflowExecutionPort for RejectingWorkflowExecutionPort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Execution> {
        Err(ApplicationError::NotFound(
            "exact ExecutionTemplate revision does not exist".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowExecutionRequest,
    ) -> ApplicationResult<Option<Execution>> {
        Ok(None)
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowExecutionRequest,
        _requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<Execution>> {
        Ok(None)
    }
}

struct FakeWorkflowCompositePort {
    engine: FlowEngine,
    children: Mutex<BTreeMap<WorkflowRunId, WorkflowRunRecord>>,
    requests: Mutex<Vec<WorkflowCompositeExecutionRequest>>,
    creates: AtomicUsize,
    terminal_on_start: bool,
    terminal_ordinals: std::collections::BTreeSet<u32>,
    start_barrier: Option<Arc<Barrier>>,
    starts_in_flight: AtomicUsize,
    maximum_starts_in_flight: AtomicUsize,
}

impl FakeWorkflowCompositePort {
    fn queued(engine: FlowEngine) -> Self {
        Self {
            engine,
            children: Mutex::new(BTreeMap::new()),
            requests: Mutex::new(Vec::new()),
            creates: AtomicUsize::new(0),
            terminal_on_start: false,
            terminal_ordinals: std::collections::BTreeSet::new(),
            start_barrier: None,
            starts_in_flight: AtomicUsize::new(0),
            maximum_starts_in_flight: AtomicUsize::new(0),
        }
    }

    fn terminal(engine: FlowEngine) -> Self {
        Self {
            terminal_on_start: true,
            ..Self::queued(engine)
        }
    }

    fn terminal_with_barrier(engine: FlowEngine, parties: usize) -> Self {
        Self {
            terminal_on_start: true,
            start_barrier: Some(Arc::new(Barrier::new(parties))),
            ..Self::queued(engine)
        }
    }

    fn terminal_ordinals(engine: FlowEngine, ordinals: impl IntoIterator<Item = u32>) -> Self {
        Self {
            terminal_ordinals: ordinals.into_iter().collect(),
            ..Self::queued(engine)
        }
    }

    fn create_count(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }

    fn maximum_starts_in_flight(&self) -> usize {
        self.maximum_starts_in_flight.load(Ordering::SeqCst)
    }

    async fn requests(&self) -> Vec<WorkflowCompositeExecutionRequest> {
        self.requests.lock().await.clone()
    }

    async fn statuses(&self) -> Vec<WorkflowRunStatus> {
        self.children
            .lock()
            .await
            .values()
            .map(|record| record.run.status)
            .collect()
    }

    async fn status_for_ordinal(&self, ordinal: u32) -> WorkflowRunStatus {
        let child_id = self
            .requests
            .lock()
            .await
            .iter()
            .find(|request| request.frame.ordinal == ordinal)
            .expect("composite frame request")
            .workflow_run_id();
        self.children
            .lock()
            .await
            .get(&child_id)
            .expect("composite child")
            .run
            .status
    }

    async fn latest_updated_at(&self) -> DateTime<Utc> {
        self.children
            .lock()
            .await
            .values()
            .map(|record| record.run.updated_at)
            .max()
            .expect("composite child")
    }

    async fn finish_cancellation(&self, at: DateTime<Utc>) {
        let mut children = self.children.lock().await;
        for record in children.values_mut() {
            if record.run.status != WorkflowRunStatus::Cancelling {
                continue;
            }
            record
                .run
                .project_flow(WorkflowRunFlowState {
                    status: WorkflowRunStatus::Cancelled,
                    flow_runtime_build_id: "a3s-cloud-workflow-execution-test@1".into(),
                    last_flow_sequence: record.run.last_flow_sequence.max(1),
                    output: None,
                    error: None,
                    started_at: Some(record.run.requested_at),
                    finished_at: Some(at),
                    observed_at: at,
                })
                .expect("finish composite child cancellation");
        }
    }

    async fn refresh_terminal_children(&self) {
        let child_ids = self
            .children
            .lock()
            .await
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for child_id in child_ids {
            let record = self
                .children
                .lock()
                .await
                .get(&child_id)
                .cloned()
                .expect("composite child");
            let snapshot = self
                .engine
                .snapshot(&record.run.flow_run_id)
                .await
                .expect("composite child snapshot");
            let history = self
                .engine
                .history(&record.run.flow_run_id)
                .await
                .expect("composite child history");
            let projected = super::project_workflow_run_record(&record, &snapshot, &history)
                .expect("project composite child")
                .expect("terminal composite child projection");
            self.children.lock().await.insert(child_id, projected);
        }
    }

    async fn build_child(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord> {
        let mut input = workflow_run_input().map_err(ApplicationError::Invalid)?;
        input.organization_id = request.frame.organization_id;
        input.project_id = request.frame.project_id;
        input.workflow_run_id = request.workflow_run_id();
        input.workflow_goal_id = request.workflow_goal_id();
        input.plan_revision_id = request.plan_revision_id();
        input.plan.workflow_definition_id = request.frame.child_workflow_definition_id;
        input.plan.workflow_revision_id = request.frame.child_workflow_revision_id;
        input.plan.workflow_digest = request.frame.child_workflow_digest.clone();
        input.plan.ontology_id = request.ontology_id;
        input.plan.ontology_revision_id = request.ontology_revision_id;
        input.plan.ontology_digest = request.ontology_digest.clone();
        input.plan.environment_id = request.environment_id;
        input.goal_input = request.frame.child_input.clone();
        input.plan.input_digest = Sha256Digest::parse(sha256_digest(
            &canonical_json_bounded(&input.goal_input, 1024 * 1024, "composite child test input")
                .map_err(ApplicationError::Invalid)?,
        ))
        .map_err(ApplicationError::Invalid)?;
        input.plan_digest = Sha256Digest::parse(sha256_digest(
            &canonical_json_bounded(
                &input.plan,
                WORKFLOW_PLAN_MAX_BYTES,
                "composite child test plan",
            )
            .map_err(ApplicationError::Invalid)?,
        ))
        .map_err(ApplicationError::Invalid)?;
        input.requested_at = request.requested_at;
        let seconds = i64::try_from(request.timeout_seconds)
            .map_err(|_| ApplicationError::Invalid("composite child timeout overflowed".into()))?;
        input.deadline_at = request
            .requested_at
            .checked_add_signed(chrono::Duration::seconds(seconds))
            .ok_or_else(|| {
                ApplicationError::Invalid("composite child deadline overflowed".into())
            })?;
        input.validate().map_err(ApplicationError::Invalid)?;
        let (run, steps) = WorkflowRun::create(input.clone(), request.requested_by)
            .map_err(ApplicationError::Invalid)?;
        let record = WorkflowRunRecord { run, steps };
        self.engine
            .start_with_id(
                input.workflow_run_id.to_string(),
                WorkflowSpec::rust_embedded(
                    &input.flow_workflow_name,
                    &input.flow_workflow_version,
                    "a3s-cloud",
                    "main",
                )
                .with_runtime_build(
                    RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1")
                        .map_err(|error| ApplicationError::Internal(error.to_string()))?,
                ),
                serde_json::to_value(&input)
                    .map_err(|error| ApplicationError::Internal(error.to_string()))?,
            )
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        if !self.terminal_on_start && !self.terminal_ordinals.contains(&request.frame.ordinal) {
            return Ok(record);
        }
        let snapshot = self
            .engine
            .snapshot(&input.workflow_run_id.to_string())
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let history = self
            .engine
            .history(&input.workflow_run_id.to_string())
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        super::project_workflow_run_record(&record, &snapshot, &history)
            .map_err(ApplicationError::Invalid)?
            .ok_or_else(|| {
                ApplicationError::Internal(
                    "composite child terminal projection was unchanged".into(),
                )
            })
    }
}

#[async_trait]
impl IWorkflowCompositeExecutionPort for FakeWorkflowCompositePort {
    async fn start_or_adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<WorkflowRunRecord> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.requests.lock().await.push(request.clone());
        let id = request.workflow_run_id();
        if let Some(record) = self.children.lock().await.get(&id).cloned() {
            return Ok(record);
        }
        if let Some(barrier) = self.start_barrier.as_ref() {
            let in_flight = self.starts_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.maximum_starts_in_flight
                .fetch_max(in_flight, Ordering::SeqCst);
            barrier.wait().await;
            self.starts_in_flight.fetch_sub(1, Ordering::SeqCst);
        }
        let record = self.build_child(request).await?;
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.children.lock().await.insert(id, record.clone());
        Ok(record)
    }

    async fn adopt(
        &self,
        request: &WorkflowCompositeExecutionRequest,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        Ok(self
            .children
            .lock()
            .await
            .get(&request.workflow_run_id())
            .cloned())
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowCompositeExecutionRequest,
        reason: Option<String>,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<WorkflowRunRecord>> {
        let mut children = self.children.lock().await;
        let Some(record) = children.get_mut(&request.workflow_run_id()) else {
            return Ok(None);
        };
        if !record.run.status.is_terminal() && record.run.status != WorkflowRunStatus::Cancelling {
            record
                .run
                .request_cancellation(reason, requested_by, requested_at)
                .map_err(ApplicationError::Conflict)?;
        }
        Ok(Some(record.clone()))
    }
}

fn execution_template(input: serde_json::Value) -> ExecutionTemplate {
    let artifact_digest = format!("sha256:{}", "a".repeat(64));
    ExecutionTemplate {
        artifact: ExecutionArtifact {
            uri: format!("oci://registry.example/a3s/function@{artifact_digest}"),
            digest: artifact_digest,
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: ExecutionProcess {
            command: vec!["/usr/bin/function".into()],
            args: vec!["invoke".into()],
            working_directory: Some("/workspace".into()),
            environment: BTreeMap::new(),
        },
        input,
        resources: ExecutionResources {
            cpu_millis: 500,
            memory_bytes: 256 * 1024 * 1024,
            pids: 128,
            ephemeral_storage_bytes: None,
            timeout_ms: 30_000,
        },
    }
}

async fn workflow_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let mut input = execution_workflow_run_input().expect("execution WorkflowRun input");
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + chrono::Duration::hours(1);
    input.validate().expect("valid execution WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1").expect("runtime build");
    let spec = WorkflowSpec::rust_embedded(
        WORKFLOW_RUN_FLOW_NAME,
        WORKFLOW_RUN_FLOW_VERSION,
        "a3s-cloud",
        "main",
    )
    .with_runtime_build(runtime_build_id.clone());
    let engine = FlowEngine::builder(Arc::new(TestFlowRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            spec,
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start WorkflowRun Flow");
    (engine, record, now)
}

async fn routed_failure_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let mut input =
        routed_execution_workflow_run_input().expect("routed execution WorkflowRun input");
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + chrono::Duration::hours(1);
    input
        .validate()
        .expect("valid routed execution WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1").expect("runtime build");
    let spec = WorkflowSpec::rust_embedded(
        WORKFLOW_RUN_FLOW_NAME,
        WORKFLOW_RUN_FLOW_VERSION_V4,
        "a3s-cloud",
        "main",
    )
    .with_runtime_build(runtime_build_id.clone());
    let engine = FlowEngine::builder(Arc::new(TestFlowRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            spec,
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start routed WorkflowRun Flow");
    (engine, record, now)
}

#[tokio::test]
async fn terminal_child_is_linked_and_resumed_into_the_parent_flow() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::terminal(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate execution")
        .expect("completed projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 1);
    let step = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_EXECUTION_STEP_ID)
        .expect("execution step projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Completed);
    let execution_output = serde_json::from_value::<WorkflowExecutionStepOutput>(
        step.result.clone().expect("execution result"),
    )
    .expect("typed execution result");
    assert_eq!(
        step.evidence_references,
        [
            format!(
                "urn:a3s:cloud:executions:execution:{}",
                execution_output.execution_id
            ),
            format!(
                "urn:a3s:cloud:operations:operation:{}",
                execution_output.operation_id
            ),
        ]
    );
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot");
    assert_eq!(snapshot.child_operations.len(), 1);
    assert!(snapshot.status.is_terminal());
}

#[tokio::test]
async fn permanent_dispatch_rejection_fails_without_creating_a_child_reference() {
    let (engine, record, now) = workflow_fixture().await;
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        Arc::new(RejectingWorkflowExecutionPort),
    );

    let failed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate rejection")
        .expect("failed projection");

    assert_eq!(failed.run.status, WorkflowRunStatus::Failed);
    assert!(failed
        .run
        .error
        .as_deref()
        .is_some_and(|error| error.contains("Execution dispatch rejected")));
    assert!(engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot")
        .child_operations
        .is_empty());
    let step = failed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_EXECUTION_STEP_ID)
        .expect("execution step projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Failed);
}

#[tokio::test]
async fn permanent_dispatch_rejection_follows_the_descriptor_bound_failure_edge() {
    let (engine, record, now) = routed_failure_workflow_fixture().await;
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        Arc::new(RejectingWorkflowExecutionPort),
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate routed rejection")
        .expect("completed projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert!(engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Flow snapshot")
        .child_operations
        .is_empty());
    let execution = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_EXECUTION_STEP_ID)
        .expect("execution step projection");
    assert_eq!(execution.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(execution.selected_handle.as_deref(), Some("error"));
    assert!(execution.evidence_references.is_empty());
    assert!(execution.result.is_none());
    assert!(execution
        .error
        .as_deref()
        .is_some_and(|error| error.contains("Execution dispatch rejected")));
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
        WorkflowStepFailureClassification::DispatchRejected
    );
    assert_eq!(
        completed
            .steps
            .iter()
            .find(|step| step.step_id == "output")
            .expect("success output projection")
            .status,
        WorkflowStepProjectionStatus::Skipped
    );
}

#[tokio::test]
async fn terminal_execution_failure_follows_the_same_typed_failure_edge() {
    let (engine, record, now) = routed_failure_workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine,
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate execution")
        .expect("waiting projection");
    let finished_at = canonical_timestamp(Utc::now() + chrono::Duration::milliseconds(1));
    port.finish(
        ExecutionOutcome::Failed {
            exit_code: Some(17),
            reason: "script failed".into(),
        },
        finished_at,
    )
    .await;

    let completed = coordinator
        .reconcile(&waiting, finished_at + chrono::Duration::milliseconds(1))
        .await
        .expect("resume failed execution")
        .expect("completed failure branch");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let execution = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_EXECUTION_STEP_ID)
        .expect("execution projection");
    assert_eq!(execution.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(execution.selected_handle.as_deref(), Some("error"));
    assert_eq!(execution.evidence_references.len(), 2);
    assert!(execution.evidence_references[0].starts_with("urn:a3s:cloud:executions:execution:"));
    assert!(execution.evidence_references[1].starts_with("urn:a3s:cloud:operations:operation:"));
    let failure = completed
        .steps
        .iter()
        .find(|step| step.step_id == "failure_output")
        .and_then(|step| step.result.clone())
        .and_then(|value| serde_json::from_value::<WorkflowStepFailureOutput>(value).ok())
        .expect("typed failure result");
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::ExecutionFailed
    );
    assert_eq!(failure.message, "script failed");
    assert!(failure.details.is_some());
}

#[tokio::test]
async fn terminal_execution_cancellation_follows_the_same_typed_failure_edge() {
    let (engine, record, now) = routed_failure_workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine,
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate execution")
        .expect("waiting projection");
    let finished_at = canonical_timestamp(Utc::now() + chrono::Duration::milliseconds(1));
    port.finish(ExecutionOutcome::Cancelled, finished_at).await;

    let completed = coordinator
        .reconcile(&waiting, finished_at + chrono::Duration::milliseconds(1))
        .await
        .expect("resume cancelled execution")
        .expect("completed cancellation branch");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let execution = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_EXECUTION_STEP_ID)
        .expect("execution projection");
    assert_eq!(execution.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(execution.selected_handle.as_deref(), Some("error"));
    assert_eq!(execution.evidence_references.len(), 2);
    let failure = completed
        .steps
        .iter()
        .find(|step| step.step_id == "failure_output")
        .and_then(|step| step.result.clone())
        .and_then(|value| serde_json::from_value::<WorkflowStepFailureOutput>(value).ok())
        .expect("typed cancellation result");
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::ExecutionCancelled
    );
    assert_eq!(failure.message, "child Execution was cancelled");
    assert!(failure.details.is_some());
}

#[tokio::test]
async fn parent_cancellation_waits_for_child_cleanup_before_flow_cancellation() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let mut waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate execution")
        .expect("waiting projection");
    let cancellation_at = canonical_timestamp(
        Utc::now().max(waiting.run.updated_at) + chrono::Duration::milliseconds(1),
    );
    waiting
        .run
        .request_cancellation(
            Some("operator requested cancellation".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request parent cancellation");

    assert!(coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(1)
        )
        .await
        .expect("coordinate child cancellation")
        .is_none());
    assert_eq!(port.status().await, ExecutionStatus::Cancelling);
    assert!(!engine
        .history(&record.run.flow_run_id)
        .await
        .expect("Flow history")
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. })));

    port.finish(
        ExecutionOutcome::Cancelled,
        canonical_timestamp(Utc::now() + chrono::Duration::milliseconds(1)),
    )
    .await;
    let cancelled = coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(2),
        )
        .await
        .expect("finish parent cancellation")
        .expect("cancelled projection");
    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.create_count(), 1);
}

#[tokio::test]
async fn parent_cancellation_before_dispatch_adopts_one_child_and_waits_for_cleanup() {
    let (engine, mut record, _) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let cancellation_at = canonical_timestamp(
        Utc::now().max(record.run.updated_at) + chrono::Duration::milliseconds(1),
    );
    record
        .run
        .request_cancellation(
            Some("operator cancelled before dispatch".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request parent cancellation before child dispatch");

    assert!(coordinator
        .reconcile(&record, cancellation_at + chrono::Duration::milliseconds(1),)
        .await
        .expect("coordinate pre-dispatch cancellation")
        .is_none());
    assert_eq!(port.create_count(), 1);
    assert_eq!(port.status().await, ExecutionStatus::Cancelling);
    assert!(!engine
        .history(&record.run.flow_run_id)
        .await
        .expect("Flow history")
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. })));

    port.finish(
        ExecutionOutcome::Cancelled,
        cancellation_at + chrono::Duration::milliseconds(2),
    )
    .await;
    let cancelled = coordinator
        .reconcile(&record, cancellation_at + chrono::Duration::milliseconds(3))
        .await
        .expect("finish pre-dispatch parent cancellation")
        .expect("cancelled projection");

    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.create_count(), 1);
}

#[tokio::test]
async fn replacement_coordinator_adopts_the_same_child_after_process_death() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowExecutionPort::queued(engine.clone()));
    let first = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let waiting = first
        .reconcile(&record, now)
        .await
        .expect("initial coordination")
        .expect("waiting projection");
    drop(first);
    port.finish(
        ExecutionOutcome::Succeeded { exit_code: 0 },
        canonical_timestamp(Utc::now() + chrono::Duration::milliseconds(1)),
    )
    .await;

    let replacement = FlowWorkflowRunCoordinator::with_executions(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowExecutionPort>,
    );
    let completed = replacement
        .reconcile(&waiting, now + chrono::Duration::milliseconds(3))
        .await
        .expect("replacement coordination")
        .expect("completed projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 1);
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("Flow snapshot")
            .child_operations
            .len(),
        1
    );
}

async fn composite_workflow_fixture(
    items: serde_json::Value,
) -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    composite_workflow_fixture_with_concurrency(items, 1).await
}

async fn composite_workflow_fixture_with_concurrency(
    items: serde_json::Value,
    maximum_concurrency: u32,
) -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let mut input = composite_workflow_run_input(
        WorkflowCompositeRegionPolicy::Iteration(WorkflowIterationRegionPolicy {
            step_id: "batch".into(),
            maximum_items: 3,
            maximum_concurrency,
            failure_mode: WorkflowIterationFailureMode::Terminate,
        }),
        items,
    )
    .expect("composite WorkflowRun input");
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + chrono::Duration::hours(1);
    input.validate().expect("valid composite WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1").expect("runtime build");
    let spec = WorkflowSpec::rust_embedded(
        &input.flow_workflow_name,
        &input.flow_workflow_version,
        "a3s-cloud",
        "main",
    )
    .with_runtime_build(runtime_build_id.clone());
    let engine = FlowEngine::builder(Arc::new(TestFlowRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            spec,
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start composite WorkflowRun Flow");
    (engine, record, now)
}

async fn application_composite_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>)
{
    let (mut input, _) = application_frame_answer_workflow_run_inputs()
        .expect("Application composite WorkflowRun input");
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + chrono::Duration::hours(1);
    input
        .validate()
        .expect("valid Application composite WorkflowRun input");
    let (run, steps) = WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id =
        RuntimeBuildId::new("a3s-cloud-workflow-execution-test@1").expect("runtime build");
    let engine = FlowEngine::builder(Arc::new(TestFlowRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id.clone()))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            WorkflowSpec::rust_embedded(
                &input.flow_workflow_name,
                &input.flow_workflow_version,
                "a3s-cloud",
                "main",
            )
            .with_runtime_build(runtime_build_id),
            serde_json::to_value(input).expect("encoded WorkflowRun input"),
        )
        .await
        .expect("start Application composite WorkflowRun Flow");
    (engine, record, now)
}

#[path = "parallel_iteration_tests.rs"]
mod parallel_iteration_tests;

#[tokio::test]
async fn terminal_composite_children_are_linked_resumed_and_adopted_per_frame() {
    let (engine, record, now) = composite_workflow_fixture(serde_json::json!([
        {"ticketId": "A", "priority": "high"},
        {"ticketId": "B", "priority": "high"}
    ]))
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::terminal(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate first composite child")
        .expect("waiting parent projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    assert_eq!(port.create_count(), 1);
    let first_child = port.requests().await[0].workflow_run_id();
    assert!(port.requests().await[0].application_frame.is_none());
    let waiting_batch = waiting
        .steps
        .iter()
        .find(|step| step.step_id == "batch")
        .expect("waiting composite step projection");
    assert_eq!(
        waiting_batch.evidence_references,
        [
            format!("urn:a3s:cloud:operations:operation:{first_child}"),
            format!("urn:a3s:cloud:workflow:workflow-run:{first_child}"),
        ]
    );
    assert_eq!(
        engine
            .snapshot(&record.run.flow_run_id)
            .await
            .expect("parent snapshot")
            .child_operations
            .len(),
        1
    );

    let completed = coordinator
        .reconcile(&waiting, now + chrono::Duration::milliseconds(1))
        .await
        .expect("coordinate second composite child")
        .expect("completed parent projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(
        completed.run.output,
        Some(serde_json::json!(["HIGH A", "HIGH B"]))
    );
    assert_eq!(port.create_count(), 2);
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("completed parent snapshot");
    assert_eq!(snapshot.child_operations.len(), 2);
    assert!(snapshot.status.is_terminal());
    let mut expected_evidence = port
        .requests()
        .await
        .into_iter()
        .flat_map(|request| {
            let child = request.workflow_run_id();
            [
                format!("urn:a3s:cloud:operations:operation:{child}"),
                format!("urn:a3s:cloud:workflow:workflow-run:{child}"),
            ]
        })
        .collect::<Vec<_>>();
    expected_evidence.sort();
    let completed_batch = completed
        .steps
        .iter()
        .find(|step| step.step_id == "batch")
        .expect("completed composite step projection");
    assert_eq!(completed_batch.evidence_references, expected_evidence);
}

#[tokio::test]
async fn v13_application_composite_projects_exact_frame_authority_to_child_port() {
    let (engine, record, now) = application_composite_workflow_fixture().await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine,
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );

    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate Application composite child")
        .expect("waiting Application parent projection");
    assert_eq!(waiting.run.status, WorkflowRunStatus::Waiting);
    let requests = port.requests().await;
    let [request] = requests.as_slice() else {
        panic!("expected one Application composite request, got {requests:#?}")
    };
    let authority = request
        .application_frame
        .as_ref()
        .expect("Application frame authority");
    authority
        .validate_for_frame(&request.frame)
        .expect("exact composite request authority");
    assert_eq!(authority.organization_id, record.run.organization_id);
    assert_eq!(authority.project_id, record.run.project_id);
    assert_eq!(authority.application_workflow_run_id, record.run.id);
    assert_eq!(authority.parent_workflow_run_id, record.run.id);
    assert_eq!(authority.frame_ordinal, 0);
    assert_eq!(authority.child_workflow_run_id, request.workflow_run_id());
}

#[tokio::test]
async fn parent_cancellation_waits_for_composite_child_workflow_terminal_state() {
    let (engine, record, now) = composite_workflow_fixture(serde_json::json!([
        {"ticketId": "A", "priority": "high"}
    ]))
    .await;
    let port = Arc::new(FakeWorkflowCompositePort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_composites(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowCompositeExecutionPort>,
    );
    let mut waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate composite child")
        .expect("waiting parent projection");
    let cancellation_at =
        canonical_timestamp(waiting.run.updated_at + chrono::Duration::milliseconds(1));
    waiting
        .run
        .request_cancellation(
            Some("operator requested cancellation".into()),
            PrincipalId::new(),
            cancellation_at,
        )
        .expect("request parent cancellation");

    assert!(coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(1),
        )
        .await
        .expect("coordinate composite child cancellation")
        .is_none());
    assert_eq!(port.statuses().await, vec![WorkflowRunStatus::Cancelling]);
    assert!(!engine
        .history(&record.run.flow_run_id)
        .await
        .expect("parent history")
        .iter()
        .any(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. })));

    port.finish_cancellation(cancellation_at + chrono::Duration::milliseconds(2))
        .await;
    let cancelled = coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(3),
        )
        .await
        .expect("finish parent cancellation")
        .expect("cancelled parent projection");
    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.create_count(), 1);
}
