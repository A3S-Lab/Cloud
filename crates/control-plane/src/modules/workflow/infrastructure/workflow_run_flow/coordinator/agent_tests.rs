use super::*;
use crate::modules::agents::{
    AgentCodeRunBinding, AgentEventContent, AgentExecution, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionStatus, AgentReleaseBinding, IWorkflowAgentPort,
    WorkflowAgentRequest, WorkflowAgentTerminalObservation, AGENT_EXECUTION_WORKFLOW_NAME,
    AGENT_EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, BuildRunId, DeploymentId, NodeId, PrincipalId, WorkloadId,
    WorkloadReplicaId, WorkloadRevisionId,
};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowAgentChildReferenceMetadata, WorkflowAgentStepOutput,
    WorkflowRun, WorkflowRunInput, WorkflowRunRecord, WorkflowRunStatus,
    WorkflowStepFailureClassification, WorkflowStepFailureOutput, WorkflowStepProjectionStatus,
    WORKFLOW_RUN_FLOW_NAME, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V9,
};
use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
use crate::modules::workflow::test_support::{
    agent_workflow_run_input, routed_agent_workflow_run_input, TEST_AGENT_STEP_ID,
};
use a3s_cloud_contracts::{AgentProtocolRunIdentityV1, AGENT_PROTOCOL_V1};
use a3s_flow::{
    FlowRuntime, RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

const TEST_RUNTIME_BUILD: &str = "a3s-cloud-workflow-agent-test@1";

#[derive(Debug, Clone, Copy)]
struct AgentTestRuntime;

#[async_trait]
impl FlowRuntime for AgentTestRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        if invocation.spec.name == AGENT_EXECUTION_WORKFLOW_NAME
            && invocation.spec.version == AGENT_EXECUTION_WORKFLOW_VERSION
        {
            return Ok(invocation
                .context()
                .complete(serde_json::json!({"test": true})));
        }
        WorkflowRunFlowRuntime::default()
            .run_workflow(invocation)
            .await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        WorkflowRunFlowRuntime::default().run_step(invocation).await
    }
}

struct FakeWorkflowAgentPort {
    engine: FlowEngine,
    request: Mutex<Option<WorkflowAgentRequest>>,
    execution: Mutex<Option<AgentExecution>>,
    creates: AtomicUsize,
    terminal_on_start: bool,
}

impl FakeWorkflowAgentPort {
    fn queued(engine: FlowEngine) -> Self {
        Self {
            engine,
            request: Mutex::new(None),
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

    fn create_count(&self) -> usize {
        self.creates.load(Ordering::SeqCst)
    }

    async fn status(&self) -> AgentExecutionStatus {
        self.execution
            .lock()
            .await
            .as_ref()
            .expect("Workflow Agent execution")
            .status
    }

    async fn finish_succeeded(&self, finished_at: DateTime<Utc>) {
        let mut stored = self.execution.lock().await;
        let execution = stored.as_mut().expect("Workflow Agent execution");
        if execution.status.is_terminal() {
            return;
        }
        let finished_at = canonical_timestamp(finished_at.max(execution.updated_at));
        if execution.status == AgentExecutionStatus::Pending {
            execution
                .start(finished_at)
                .expect("start Workflow Agent execution");
        }
        execution
            .apply_event(&agent_event(
                AgentExecutionEventKind::ModelOutput,
                serde_json::json!({"text": "agent answer"}),
                finished_at,
            ))
            .expect("apply Agent model output");
        execution
            .apply_event(&agent_event(
                AgentExecutionEventKind::ExecutionCompleted,
                serde_json::json!({}),
                finished_at,
            ))
            .expect("complete Workflow Agent execution");
    }

    async fn finish_cancelled(&self, finished_at: DateTime<Utc>) {
        let mut stored = self.execution.lock().await;
        let execution = stored.as_mut().expect("Workflow Agent execution");
        let finished_at = canonical_timestamp(finished_at.max(execution.updated_at));
        execution
            .apply_event(&agent_event(
                AgentExecutionEventKind::ExecutionCancelled,
                serde_json::json!({}),
                finished_at,
            ))
            .expect("cancel Workflow Agent execution");
    }

    async fn finish_failed(&self, reason: &str, finished_at: DateTime<Utc>) {
        let mut stored = self.execution.lock().await;
        let execution = stored.as_mut().expect("Workflow Agent execution");
        let finished_at = canonical_timestamp(finished_at.max(execution.updated_at));
        if execution.status == AgentExecutionStatus::Pending {
            execution
                .start(finished_at)
                .expect("start Workflow Agent execution");
        }
        execution
            .apply_event(&agent_event(
                AgentExecutionEventKind::ExecutionFailed,
                serde_json::json!({"reason": reason}),
                finished_at,
            ))
            .expect("fail Workflow Agent execution");
    }

    async fn ensure_request(&self, request: &WorkflowAgentRequest) -> ApplicationResult<()> {
        let observed = self.request.lock().await;
        if observed
            .as_ref()
            .is_some_and(|existing| existing != request)
        {
            return Err(ApplicationError::Conflict(
                "Workflow Agent request authority drifted".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IWorkflowAgentPort for FakeWorkflowAgentPort {
    async fn start_or_adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentExecution> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.ensure_request(request).await?;
        let mut stored = self.execution.lock().await;
        if let Some(execution) = stored.as_ref() {
            return Ok(execution.clone());
        }
        let release = AgentReleaseBinding::new(
            request.organization_id,
            request.agent_asset_id,
            request.agent_asset_release_id,
            BuildRunId::new(),
            format!(
                "oci://registry.example/agents/workflow@{}",
                request.agent_release_digest
            ),
            request.agent_release_digest.clone(),
            "application/vnd.oci.image.manifest.v1+json",
            1,
        )
        .map_err(ApplicationError::Invalid)?;
        let mut execution = AgentExecution::create(
            request.organization_id,
            crate::modules::shared_kernel::domain::AgentConversationId::new(),
            crate::modules::shared_kernel::domain::AgentExecutionId::new(),
            crate::modules::shared_kernel::domain::OperationId::new(),
            release,
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        let binding = AgentCodeRunBinding::new(
            NodeId::new(),
            WorkloadId::new(),
            WorkloadRevisionId::new(),
            DeploymentId::new(),
            WorkloadReplicaId::new(),
            "agent-runtime:workflow-test",
            1,
            request.agent_release_digest.clone(),
            "agent",
            AgentProtocolRunIdentityV1 {
                schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
                protocol: AGENT_PROTOCOL_V1.into(),
                agent_release_identity: request.agent_release_digest.to_string(),
                session_id: format!("workflow-agent-session-{}", execution.conversation_id),
                run_id: format!("workflow-agent-execution-{}", execution.id),
            },
            request.requested_at,
        )
        .map_err(ApplicationError::Invalid)?;
        execution
            .bind_code_run(binding)
            .map_err(ApplicationError::Invalid)?;
        self.engine
            .start_with_id(
                execution.operation_id.to_string(),
                WorkflowSpec::rust_embedded(
                    AGENT_EXECUTION_WORKFLOW_NAME,
                    AGENT_EXECUTION_WORKFLOW_VERSION,
                    "a3s-cloud",
                    "main",
                )
                .with_runtime_build(
                    RuntimeBuildId::new(TEST_RUNTIME_BUILD)
                        .map_err(|error| ApplicationError::Internal(error.to_string()))?,
                ),
                serde_json::json!({
                    "organizationId": execution.organization_id,
                    "executionId": execution.id,
                }),
            )
            .await
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        self.creates.fetch_add(1, Ordering::SeqCst);
        *self.request.lock().await = Some(request.clone());
        *stored = Some(execution.clone());
        drop(stored);
        if self.terminal_on_start {
            self.finish_succeeded(request.requested_at + chrono::Duration::milliseconds(1))
                .await;
            return Ok(self
                .execution
                .lock()
                .await
                .as_ref()
                .expect("terminal Workflow Agent execution")
                .clone());
        }
        Ok(execution)
    }

    async fn adopt(
        &self,
        request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentExecution>> {
        request.validate().map_err(ApplicationError::Invalid)?;
        self.ensure_request(request).await?;
        Ok(self.execution.lock().await.clone())
    }

    async fn request_cancellation(
        &self,
        request: &WorkflowAgentRequest,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<AgentExecution>> {
        self.ensure_request(request).await?;
        let mut stored = self.execution.lock().await;
        let Some(execution) = stored.as_mut() else {
            return Ok(None);
        };
        if !execution.status.is_terminal() && execution.status != AgentExecutionStatus::Cancelling {
            execution
                .request_cancellation(requested_at.max(execution.updated_at))
                .map_err(ApplicationError::Conflict)?;
        }
        Ok(Some(execution.clone()))
    }

    async fn terminal_observation(
        &self,
        request: &WorkflowAgentRequest,
        execution: &AgentExecution,
    ) -> ApplicationResult<Option<WorkflowAgentTerminalObservation>> {
        self.ensure_request(request).await?;
        let current = self.execution.lock().await.clone();
        let Some(current) = current else {
            return Ok(None);
        };
        if &current != execution {
            return Err(ApplicationError::Conflict(
                "stale Workflow Agent terminal observation".into(),
            ));
        }
        Ok(current
            .status
            .is_terminal()
            .then_some(WorkflowAgentTerminalObservation {
                execution: current,
                output_text: "agent answer".into(),
                terminal_event_sequence: 3,
            }))
    }
}

struct RejectingWorkflowAgentPort;

#[async_trait]
impl IWorkflowAgentPort for RejectingWorkflowAgentPort {
    async fn start_or_adopt(
        &self,
        _request: &WorkflowAgentRequest,
    ) -> ApplicationResult<AgentExecution> {
        Err(ApplicationError::NotFound(
            "exact Agent release does not exist".into(),
        ))
    }

    async fn adopt(
        &self,
        _request: &WorkflowAgentRequest,
    ) -> ApplicationResult<Option<AgentExecution>> {
        Ok(None)
    }

    async fn request_cancellation(
        &self,
        _request: &WorkflowAgentRequest,
        _requested_at: DateTime<Utc>,
    ) -> ApplicationResult<Option<AgentExecution>> {
        Ok(None)
    }

    async fn terminal_observation(
        &self,
        _request: &WorkflowAgentRequest,
        _execution: &AgentExecution,
    ) -> ApplicationResult<Option<WorkflowAgentTerminalObservation>> {
        Ok(None)
    }
}

fn agent_event(
    kind: AgentExecutionEventKind,
    content: serde_json::Value,
    occurred_at: DateTime<Utc>,
) -> AgentExecutionEventDraft {
    AgentExecutionEventDraft::new(
        kind,
        AgentEventContent::inline_json(content).expect("Agent event content"),
        occurred_at,
    )
    .expect("Agent event")
}

async fn workflow_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    workflow_fixture_with(agent_workflow_run_input().expect("Agent WorkflowRun input")).await
}

async fn routed_workflow_fixture() -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    workflow_fixture_with(
        routed_agent_workflow_run_input().expect("routed Agent WorkflowRun input"),
    )
    .await
}

async fn workflow_fixture_with(
    mut input: WorkflowRunInput,
) -> (FlowEngine, WorkflowRunRecord, DateTime<Utc>) {
    let now = canonical_timestamp(Utc::now());
    input.requested_at = now;
    input.deadline_at = now + chrono::Duration::hours(1);
    input.validate().expect("valid Agent WorkflowRun input");
    let (run, steps) =
        WorkflowRun::create(input.clone(), PrincipalId::new()).expect("Agent WorkflowRun");
    let record = WorkflowRunRecord { run, steps };
    let runtime_build_id = RuntimeBuildId::new(TEST_RUNTIME_BUILD).expect("runtime build");
    let spec = WorkflowSpec::rust_embedded(
        WORKFLOW_RUN_FLOW_NAME,
        &input.flow_workflow_version,
        "a3s-cloud",
        "main",
    )
    .with_runtime_build(runtime_build_id.clone());
    let engine = FlowEngine::builder(Arc::new(AgentTestRuntime))
        .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(runtime_build_id))
        .build();
    engine
        .start_with_id(
            input.workflow_run_id.to_string(),
            spec,
            serde_json::to_value(input).expect("encoded Agent WorkflowRun input"),
        )
        .await
        .expect("start Agent WorkflowRun Flow");
    (engine, record, now)
}

#[tokio::test]
async fn terminal_agent_is_linked_resumed_and_projected_with_exact_evidence() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowAgentPort::terminal(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate Workflow Agent")
        .expect("completed Agent WorkflowRun projection");

    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 1);
    let step = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_AGENT_STEP_ID)
        .expect("Agent step projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Completed);
    let output = serde_json::from_value::<WorkflowAgentStepOutput>(
        step.result.clone().expect("Agent result"),
    )
    .expect("typed Agent result");
    assert_eq!(output.text, "agent answer");
    assert!(output.provider.is_some());
    assert_eq!(
        step.evidence_references,
        [
            format!(
                "urn:a3s:cloud:agents:conversation:{}",
                output.conversation_id
            ),
            format!(
                "urn:a3s:cloud:agents:execution:{}",
                output.agent_execution_id
            ),
            format!("urn:a3s:cloud:operations:operation:{}", output.operation_id),
        ]
    );
    let snapshot = engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Agent WorkflowRun snapshot");
    let child = snapshot
        .child_operations
        .values()
        .next()
        .expect("Agent child reference");
    assert_eq!(child.kind, "agent_execution");
    let metadata =
        serde_json::from_value::<WorkflowAgentChildReferenceMetadata>(child.metadata.clone())
            .expect("Agent child metadata");
    assert_eq!(metadata.conversation_id, output.conversation_id);
    assert_eq!(metadata.agent_execution_id, output.agent_execution_id);
    assert_eq!(metadata.operation_id, output.operation_id);
}

#[tokio::test]
async fn replacement_coordinator_adopts_the_same_agent_child_after_restart() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowAgentPort::queued(engine.clone()));
    let first = FlowWorkflowRunCoordinator::with_agents(
        engine.clone(),
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );
    let waiting = first
        .reconcile(&record, now)
        .await
        .expect("initial Agent coordination")
        .expect("waiting Agent projection");
    drop(first);
    port.finish_succeeded(now + chrono::Duration::seconds(1))
        .await;

    let replacement = FlowWorkflowRunCoordinator::with_agents(
        engine,
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );
    let completed = replacement
        .reconcile(&waiting, now + chrono::Duration::seconds(2))
        .await
        .expect("replacement Agent coordination")
        .expect("completed replacement projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert_eq!(port.create_count(), 1);
}

#[tokio::test]
async fn parent_cancellation_waits_for_agent_terminal_cleanup() {
    let (engine, record, now) = workflow_fixture().await;
    let port = Arc::new(FakeWorkflowAgentPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine,
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );
    let mut waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("initial Agent coordination")
        .expect("waiting Agent projection");
    let cancellation_at =
        canonical_timestamp(now.max(waiting.run.updated_at) + chrono::Duration::milliseconds(1));
    waiting
        .run
        .request_cancellation(
            Some("operator cancelled Agent WorkflowRun".into()),
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
        .expect("coordinate Agent cancellation")
        .is_none());
    assert_eq!(port.status().await, AgentExecutionStatus::Cancelling);
    port.finish_cancelled(cancellation_at + chrono::Duration::milliseconds(2))
        .await;
    let cancelled = coordinator
        .reconcile(
            &waiting,
            cancellation_at + chrono::Duration::milliseconds(3),
        )
        .await
        .expect("finish Agent cancellation")
        .expect("cancelled Agent WorkflowRun projection");
    assert_eq!(cancelled.run.status, WorkflowRunStatus::Cancelled);
    assert_eq!(port.create_count(), 1);
    let step = cancelled
        .steps
        .iter()
        .find(|step| step.step_id == TEST_AGENT_STEP_ID)
        .expect("cancelled Agent projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Cancelled);
    assert_eq!(step.evidence_references.len(), 3);
    assert!(step.evidence_references[0].starts_with("urn:a3s:cloud:agents:conversation:"));
    assert!(step.evidence_references[1].starts_with("urn:a3s:cloud:agents:execution:"));
    assert!(step.evidence_references[2].starts_with("urn:a3s:cloud:operations:operation:"));
}

#[tokio::test]
async fn permanent_agent_dispatch_rejection_fails_without_a_child_reference() {
    let (engine, record, now) = workflow_fixture().await;
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine.clone(),
        Arc::new(RejectingWorkflowAgentPort),
    );

    let failed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate Agent rejection")
        .expect("failed Agent WorkflowRun projection");
    assert_eq!(failed.run.status, WorkflowRunStatus::Failed);
    assert!(failed
        .run
        .error
        .as_deref()
        .is_some_and(|error| error.contains("Agent dispatch rejected")));
    assert!(engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("Agent WorkflowRun snapshot")
        .child_operations
        .is_empty());
    let step = failed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_AGENT_STEP_ID)
        .expect("failed Agent projection");
    assert_eq!(step.status, WorkflowStepProjectionStatus::Failed);
}

#[tokio::test]
async fn permanent_agent_dispatch_rejection_follows_the_descriptor_bound_failure_edge() {
    let (engine, record, now) = routed_workflow_fixture().await;
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine.clone(),
        Arc::new(RejectingWorkflowAgentPort),
    );

    let completed = coordinator
        .reconcile(&record, now)
        .await
        .expect("coordinate routed Agent rejection")
        .expect("completed Agent failure branch projection");
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    assert!(engine
        .snapshot(&record.run.flow_run_id)
        .await
        .expect("routed Agent WorkflowRun snapshot")
        .child_operations
        .is_empty());

    let agent = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_AGENT_STEP_ID)
        .expect("failed Agent projection");
    assert_eq!(agent.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(agent.selected_handle.as_deref(), Some("error"));
    assert!(agent.result.is_none());
    assert!(agent.evidence_references.is_empty());
    assert_eq!(agent.error.as_deref(), Some("Agent dispatch was rejected"));
    assert!(!agent
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("exact Agent release does not exist"));

    let failure = completed
        .steps
        .iter()
        .find(|step| step.step_id == "failure_output")
        .and_then(|step| step.result.clone())
        .and_then(|result| serde_json::from_value::<WorkflowStepFailureOutput>(result).ok())
        .expect("typed Agent failure branch output");
    assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V9);
    assert_eq!(
        failure.classification,
        WorkflowStepFailureClassification::AgentDispatchRejected
    );
    assert_eq!(failure.message, "Agent dispatch was rejected");
    assert!(failure.details.is_none());
}

#[tokio::test]
async fn terminal_agent_failure_follows_the_same_typed_failure_edge() {
    let (engine, record, now) = routed_workflow_fixture().await;
    let port = Arc::new(FakeWorkflowAgentPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine,
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );
    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("start routed Agent")
        .expect("waiting routed Agent projection");
    let finished_at = now + chrono::Duration::seconds(1);
    port.finish_failed("private provider terminal failure", finished_at)
        .await;

    let completed = coordinator
        .reconcile(&waiting, finished_at + chrono::Duration::milliseconds(1))
        .await
        .expect("resume failed Agent")
        .expect("completed Agent failure branch");
    assert_routed_terminal_agent_failure(
        &completed,
        WorkflowStepFailureClassification::AgentExecutionFailed,
        "Agent execution failed",
        "private provider terminal failure",
    );
}

#[tokio::test]
async fn terminal_agent_cancellation_follows_the_same_typed_failure_edge() {
    let (engine, record, now) = routed_workflow_fixture().await;
    let port = Arc::new(FakeWorkflowAgentPort::queued(engine.clone()));
    let coordinator = FlowWorkflowRunCoordinator::with_agents(
        engine,
        port.clone() as Arc<dyn IWorkflowAgentPort>,
    );
    let waiting = coordinator
        .reconcile(&record, now)
        .await
        .expect("start routed Agent")
        .expect("waiting routed Agent projection");
    let finished_at = now + chrono::Duration::seconds(1);
    port.finish_cancelled(finished_at).await;

    let completed = coordinator
        .reconcile(&waiting, finished_at + chrono::Duration::milliseconds(1))
        .await
        .expect("resume cancelled Agent")
        .expect("completed Agent cancellation branch");
    assert_routed_terminal_agent_failure(
        &completed,
        WorkflowStepFailureClassification::AgentExecutionCancelled,
        "Agent execution was cancelled",
        "child Agent execution was cancelled",
    );
}

fn assert_routed_terminal_agent_failure(
    completed: &WorkflowRunRecord,
    classification: WorkflowStepFailureClassification,
    stable_message: &str,
    private_error: &str,
) {
    assert_eq!(completed.run.status, WorkflowRunStatus::Completed);
    let agent = completed
        .steps
        .iter()
        .find(|step| step.step_id == TEST_AGENT_STEP_ID)
        .expect("failed Agent projection");
    assert_eq!(agent.status, WorkflowStepProjectionStatus::Failed);
    assert_eq!(agent.selected_handle.as_deref(), Some("error"));
    assert_eq!(agent.error.as_deref(), Some(stable_message));
    assert!(!agent
        .error
        .as_deref()
        .unwrap_or_default()
        .contains(private_error));
    assert!(agent.result.is_none());
    assert_eq!(agent.evidence_references.len(), 3);
    assert!(agent.evidence_references[0].starts_with("urn:a3s:cloud:agents:conversation:"));
    assert!(agent.evidence_references[1].starts_with("urn:a3s:cloud:agents:execution:"));
    assert!(agent.evidence_references[2].starts_with("urn:a3s:cloud:operations:operation:"));

    let failure = completed
        .steps
        .iter()
        .find(|step| step.step_id == "failure_output")
        .and_then(|step| step.result.clone())
        .and_then(|result| serde_json::from_value::<WorkflowStepFailureOutput>(result).ok())
        .expect("typed Agent terminal failure branch output");
    assert_eq!(failure.schema, WORKFLOW_STEP_FAILURE_OUTPUT_SCHEMA_V9);
    assert_eq!(failure.classification, classification);
    assert_eq!(failure.message, stable_message);
    assert!(!serde_json::to_string(&failure)
        .expect("encoded Agent failure")
        .contains("private"));
}
