use super::project_workflow_run_record;
use super::projection::{execution_hook, verify_flow_authority};
use crate::modules::executions::{
    Execution, ExecutionOutcome, IWorkflowExecutionPort, WorkflowExecutionRequest,
    EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::shared_kernel::application::ApplicationError;
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, WorkflowExecutionChildReferenceMetadata,
    WorkflowExecutionHookMetadata, WorkflowExecutionOutcome, WorkflowExecutionResumePayload,
    WorkflowExecutionStepOutput, WorkflowRunCoordinationError, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowStepKind, WORKFLOW_EXECUTION_RESULT_SCHEMA,
};
use a3s_flow::{
    CancellationRequest, ChildOperationReference, FlowEngine, FlowError, FlowEvent, HookStatus,
    WorkflowRunSnapshot,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

#[derive(Clone)]
pub struct FlowWorkflowRunCoordinator {
    engine: FlowEngine,
    executions: Option<Arc<dyn IWorkflowExecutionPort>>,
}

impl FlowWorkflowRunCoordinator {
    pub const fn new(engine: FlowEngine) -> Self {
        Self {
            engine,
            executions: None,
        }
    }

    pub fn with_executions(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
        }
    }

    async fn coordinate_active_execution(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
    ) -> Result<(), WorkflowRunCoordinationError> {
        let hooks = execution_hooks(record, snapshot, history)?;
        let active = hooks
            .into_iter()
            .filter(|hook| hook.status == HookStatus::Active)
            .collect::<Vec<_>>();
        if active.len() > 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "WorkflowRun replay exposed more than one active Execution hook".into(),
            ));
        }
        let Some(hook) = active.first() else {
            return Ok(());
        };
        let port = self.executions.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Execution coordination is not configured".into(),
            )
        })?;
        let request = hook.request();
        let execution = match port.start_or_adopt(&request).await {
            Ok(execution) => execution,
            Err(error) if permanent_dispatch_error(&error) => {
                let payload = WorkflowExecutionResumePayload::rejected(
                    &hook.metadata,
                    rejection_reason(&error),
                )
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
                self.engine
                    .resume_hook(
                        &record.run.flow_run_id,
                        &hook.metadata.flow_hook_id(),
                        serde_json::to_value(payload).map_err(|error| {
                            WorkflowRunCoordinationError::Unavailable(error.to_string())
                        })?,
                    )
                    .await
                    .map_err(|error| unavailable_at("reject Execution hook", error))?;
                return Ok(());
            }
            Err(error) => return Err(application_unavailable(error)),
        };
        let linked = self
            .link_child(record, snapshot, &hook.metadata, &execution)
            .await?;
        if linked && execution.status.is_terminal() {
            self.resume_terminal_execution(record, &hook.metadata, &execution)
                .await?;
        }
        Ok(())
    }

    async fn cancel_execution_children(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        history: &[a3s_flow::FlowEventEnvelope],
        requested_at: DateTime<Utc>,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let hooks = execution_hooks(record, snapshot, history)?;
        if hooks.is_empty() {
            return Ok(true);
        }
        let port = self.executions.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "Workflow Execution coordination is not configured".into(),
            )
        })?;
        let mut all_terminal = true;
        for hook in hooks {
            let request = hook.request();
            let mut execution = match port
                .adopt(&request)
                .await
                .map_err(application_unavailable)?
            {
                Some(execution) => execution,
                None => match port.start_or_adopt(&request).await {
                    Ok(execution) => execution,
                    // No child can be admitted for this immutable request, so there is
                    // nothing to clean up. Conflicts remain fail-closed because they can
                    // also mean an existing child no longer matches the pinned authority.
                    Err(
                        ApplicationError::Invalid(_)
                        | ApplicationError::NotFound(_)
                        | ApplicationError::Forbidden(_),
                    ) => continue,
                    Err(error) => return Err(application_unavailable(error)),
                },
            };
            let linked = self
                .link_child(record, snapshot, &hook.metadata, &execution)
                .await?;
            if !execution.status.is_terminal() {
                let cancellation_at = canonical_timestamp(requested_at.max(execution.updated_at));
                execution = port
                    .request_cancellation(&request, cancellation_at)
                    .await
                    .map_err(application_unavailable)?
                    .ok_or_else(|| {
                        WorkflowRunCoordinationError::Unavailable(
                            "Workflow child Execution disappeared during cancellation".into(),
                        )
                    })?;
            }
            all_terminal &= linked && execution.status.is_terminal();
        }
        Ok(all_terminal)
    }

    async fn link_child(
        &self,
        record: &WorkflowRunRecord,
        snapshot: &WorkflowRunSnapshot,
        hook: &WorkflowExecutionHookMetadata,
        execution: &Execution,
    ) -> Result<bool, WorkflowRunCoordinationError> {
        let invocation_template_digest = Sha256Digest::parse(&execution.template_digest)
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let metadata =
            WorkflowExecutionChildReferenceMetadata::new(hook, invocation_template_digest)
                .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let child = ChildOperationReference::new(
            hook.flow_hook_id(),
            "execution",
            execution.operation_id.to_string(),
        )
        .with_flow_run_id(execution.operation_id.to_string())
        .with_metadata(
            serde_json::to_value(metadata)
                .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?,
        );
        if snapshot.status.is_terminal() {
            return match snapshot.child_operations.get(&child.reference_id) {
                Some(existing) if existing == &child => Ok(true),
                Some(_) => Err(WorkflowRunCoordinationError::Unavailable(
                    "terminal WorkflowRun child Execution reference drifted".into(),
                )),
                // Runs terminated by older coordinators cannot be amended. The child is still
                // cancelled and awaited before their terminal projection is committed.
                None => Ok(true),
            };
        }
        let child_snapshot = match self
            .engine
            .snapshot(&execution.operation_id.to_string())
            .await
        {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => return Ok(false),
            Err(error) => return Err(unavailable_at("read child Execution Flow identity", error)),
        };
        if child_snapshot.run_id != execution.operation_id.to_string()
            || child_snapshot.spec.name != EXECUTION_WORKFLOW_NAME
            || child_snapshot.spec.version != EXECUTION_WORKFLOW_VERSION
        {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow child Execution Flow identity drifted".into(),
            ));
        }
        self.engine
            .link_child_operation(&record.run.flow_run_id, child)
            .await
            .map_err(|error| unavailable_at("link child Execution", error))
            .map(|()| true)
    }

    async fn resume_terminal_execution(
        &self,
        record: &WorkflowRunRecord,
        hook: &WorkflowExecutionHookMetadata,
        execution: &Execution,
    ) -> Result<(), WorkflowRunCoordinationError> {
        let outcome = match execution.outcome.as_ref().ok_or_else(|| {
            WorkflowRunCoordinationError::Unavailable(
                "terminal Workflow child Execution has no outcome".into(),
            )
        })? {
            ExecutionOutcome::Succeeded { exit_code } => WorkflowExecutionOutcome::Succeeded {
                exit_code: *exit_code,
            },
            ExecutionOutcome::Failed { exit_code, reason } => WorkflowExecutionOutcome::Failed {
                exit_code: *exit_code,
                reason: reason.clone(),
            },
            ExecutionOutcome::Cancelled => WorkflowExecutionOutcome::Cancelled,
        };
        let output = WorkflowExecutionStepOutput {
            schema: WORKFLOW_EXECUTION_RESULT_SCHEMA.into(),
            execution_id: execution.id,
            operation_id: execution.operation_id,
            execution_template_id: hook.execution_template_id,
            execution_template_revision_id: hook.execution_template_revision_id,
            execution_template_digest: hook.execution_template_digest.clone(),
            invocation_template_digest: Sha256Digest::parse(&execution.template_digest)
                .map_err(WorkflowRunCoordinationError::Unavailable)?,
            outcome,
            finished_at: execution.finished_at.ok_or_else(|| {
                WorkflowRunCoordinationError::Unavailable(
                    "terminal Workflow child Execution has no finish time".into(),
                )
            })?,
        };
        let payload = WorkflowExecutionResumePayload::new(hook, output)
            .map_err(WorkflowRunCoordinationError::Unavailable)?;
        self.engine
            .resume_hook(
                &record.run.flow_run_id,
                &hook.flow_hook_id(),
                serde_json::to_value(payload).map_err(|error| {
                    WorkflowRunCoordinationError::Unavailable(error.to_string())
                })?,
            )
            .await
            .map_err(|error| unavailable_at("resume terminal child Execution", error))
    }
}

#[async_trait]
impl IWorkflowRunCoordinator for FlowWorkflowRunCoordinator {
    async fn reconcile(
        &self,
        record: &WorkflowRunRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowRunCoordinationError> {
        let mut snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => {
                return Err(WorkflowRunCoordinationError::Deferred(
                    "the correlated Operation has not started A3S Flow".into(),
                ))
            }
            Err(error) => return Err(unavailable(error)),
        };
        let mut history = self
            .engine
            .history(&record.run.flow_run_id)
            .await
            .map_err(unavailable)?;
        if let Err(error) = verify_flow_authority(record, &snapshot, &history) {
            return project_drift(record, &snapshot, &history, error).map(Some);
        }
        let cancelling = record.run.status == WorkflowRunStatus::Cancelling;
        let timed_out = !cancelling && now >= record.run.execution_input.deadline_at;
        if cancelling || timed_out {
            if !self
                .cancel_execution_children(record, &snapshot, &history, now)
                .await?
            {
                return Ok(None);
            }
            if !snapshot.status.is_terminal() {
                if cancelling {
                    self.engine
                        .request_cancellation(
                            &record.run.flow_run_id,
                            CancellationRequest::new(record.run.cancellation_reason.clone()),
                        )
                        .await
                        .map_err(unavailable)?;
                } else {
                    self.engine
                        .terminate_for_timeout(
                            &record.run.flow_run_id,
                            record.run.execution_input.deadline_at,
                            Some("WorkflowRun exceeded its immutable deadline".into()),
                        )
                        .await
                        .map_err(unavailable)?;
                }
            }
        } else if !snapshot.status.is_terminal() {
            self.coordinate_active_execution(record, &snapshot, &history)
                .await?;
        }
        snapshot = self
            .engine
            .snapshot(&record.run.flow_run_id)
            .await
            .map_err(|error| unavailable_at("refresh WorkflowRun snapshot", error))?;
        history = self
            .engine
            .history(&record.run.flow_run_id)
            .await
            .map_err(|error| unavailable_at("refresh WorkflowRun history", error))?;
        match project_workflow_run_record(record, &snapshot, &history) {
            Ok(projected) => Ok(projected),
            Err(error) => project_drift(record, &snapshot, &history, error).map(Some),
        }
    }
}

#[derive(Debug, Clone)]
struct ObservedExecutionHook {
    metadata: WorkflowExecutionHookMetadata,
    created_at: DateTime<Utc>,
    status: HookStatus,
}

impl ObservedExecutionHook {
    fn request(&self) -> WorkflowExecutionRequest {
        WorkflowExecutionRequest {
            organization_id: self.metadata.organization_id,
            project_id: self.metadata.project_id,
            environment_id: self.metadata.environment_id,
            workflow_run_id: self.metadata.workflow_run_id,
            plan_revision_id: self.metadata.plan_revision_id,
            plan_digest: self.metadata.plan_digest.clone(),
            step_id: self.metadata.step_id.clone(),
            step_attempt: self.metadata.step_attempt,
            execution_template_id: self.metadata.execution_template_id,
            execution_template_revision_id: self.metadata.execution_template_revision_id,
            execution_template_digest: self.metadata.execution_template_digest.clone(),
            capability: self.metadata.capability.clone(),
            input: self.metadata.effective_input.clone(),
            requested_at: self.created_at,
        }
    }
}

fn execution_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<ObservedExecutionHook>, WorkflowRunCoordinationError> {
    let mut hooks = Vec::new();
    for resolved in record
        .run
        .execution_input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?
    {
        if resolved.plan.kind != WorkflowStepKind::Execution {
            continue;
        }
        let Some((hook, metadata)) =
            execution_hook(&record.run.execution_input, &resolved, snapshot)
                .map_err(WorkflowRunCoordinationError::Unavailable)?
        else {
            continue;
        };
        let expected_metadata = serde_json::to_value(&metadata)
            .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
        let matching = history
            .iter()
            .filter(|envelope| {
                matches!(
                    &envelope.event,
                    FlowEvent::HookCreated { hook_id, .. }
                        if hook_id == &metadata.flow_hook_id()
                )
            })
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow execution hook {:?} must have exactly one creation event",
                metadata.flow_hook_id()
            )));
        }
        let FlowEvent::HookCreated {
            token,
            metadata: observed_metadata,
            ..
        } = &matching[0].event
        else {
            unreachable!("matching event was filtered to HookCreated")
        };
        if token != &metadata.flow_hook_token() || observed_metadata != &expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow execution hook {:?} creation authority drifted",
                metadata.flow_hook_id()
            )));
        }
        hooks.push(ObservedExecutionHook {
            metadata,
            created_at: canonical_timestamp(matching[0].timestamp),
            status: hook.status,
        });
    }
    Ok(hooks)
}

fn permanent_dispatch_error(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::Invalid(_)
            | ApplicationError::NotFound(_)
            | ApplicationError::Conflict(_)
            | ApplicationError::Forbidden(_)
    )
}

fn rejection_reason(error: &ApplicationError) -> String {
    let sanitized = error
        .to_string()
        .replace(['\0', '\r', '\n'], " ")
        .chars()
        .take(8 * 1024)
        .collect::<String>();
    format!("Execution dispatch rejected: {sanitized}")
}

fn application_unavailable(error: ApplicationError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(error.to_string())
}

fn project_drift(
    record: &WorkflowRunRecord,
    snapshot: &a3s_flow::WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
    error: String,
) -> Result<WorkflowRunRecord, WorkflowRunCoordinationError> {
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .unwrap_or_else(Utc::now);
    let started_at = history.iter().find_map(|event| {
        matches!(event.event, a3s_flow::FlowEvent::RunStarted).then_some(event.timestamp)
    });
    let build_id = snapshot
        .spec
        .runtime_build_id
        .as_ref()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "unpinned-flow-runtime".into());
    let mut projected = record.clone();
    projected
        .run
        .project_flow(crate::modules::workflow::domain::WorkflowRunFlowState {
            status: WorkflowRunStatus::Failed,
            flow_runtime_build_id: build_id,
            last_flow_sequence: snapshot.last_sequence.max(1),
            output: None,
            error: Some(format!("WorkflowRun replay drift: {error}")),
            started_at,
            finished_at: Some(observed_at),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    for step in &mut projected.steps {
        if step.status.is_terminal() {
            continue;
        }
        step.project_flow(crate::modules::workflow::domain::WorkflowStepFlowState {
            status: crate::modules::workflow::domain::WorkflowStepProjectionStatus::Cancelled,
            attempt_generation: step.attempt_generation,
            selected_handle: None,
            result: None,
            error: None,
            last_flow_sequence: snapshot.last_sequence.max(1),
            observed_at,
        })
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    }
    projected
        .validate()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    Ok(projected)
}

fn unavailable(error: FlowError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(error.to_string())
}

fn unavailable_at(operation: &str, error: FlowError) -> WorkflowRunCoordinationError {
    WorkflowRunCoordinationError::Unavailable(format!("could not {operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::{
        ExecutionArtifact, ExecutionProcess, ExecutionResources, ExecutionStatus,
        ExecutionTemplate, WorkflowExecutionBinding,
    };
    use crate::modules::shared_kernel::application::ApplicationResult;
    use crate::modules::shared_kernel::domain::{ExecutionId, PrincipalId};
    use crate::modules::workflow::domain::{
        IWorkflowRunCoordinator, WorkflowRun, WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME,
        WORKFLOW_RUN_FLOW_VERSION,
    };
    use crate::modules::workflow::infrastructure::WorkflowRunFlowRuntime;
    use crate::modules::workflow::test_support::{
        execution_workflow_run_input, TEST_EXECUTION_STEP_ID,
    };
    use a3s_flow::{
        FlowEvent, FlowRuntime, RuntimeBuildCompatibility, RuntimeBuildId, RuntimeCommand,
        StepInvocation, WorkflowInvocation, WorkflowSpec,
    };
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

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
            WorkflowRunFlowRuntime.run_workflow(invocation).await
        }

        async fn run_step(
            &self,
            invocation: StepInvocation,
        ) -> Result<serde_json::Value, FlowError> {
            WorkflowRunFlowRuntime.run_step(invocation).await
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
        let (run, steps) =
            WorkflowRun::create(input.clone(), PrincipalId::new()).expect("WorkflowRun");
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
}
