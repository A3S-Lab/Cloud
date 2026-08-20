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

mod composite;
mod connector;

#[derive(Clone)]
pub struct FlowWorkflowRunCoordinator {
    engine: FlowEngine,
    executions: Option<Arc<dyn IWorkflowExecutionPort>>,
    composites: Option<Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>>,
    connectors: Option<Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>>,
}

impl FlowWorkflowRunCoordinator {
    pub const fn new(engine: FlowEngine) -> Self {
        Self {
            engine,
            executions: None,
            composites: None,
            connectors: None,
        }
    }

    pub fn with_executions(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: None,
            connectors: None,
        }
    }

    pub fn with_ports(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: Some(composites),
            connectors: None,
        }
    }

    pub fn with_all_ports(
        engine: FlowEngine,
        executions: Arc<dyn IWorkflowExecutionPort>,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
        connectors: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>,
    ) -> Self {
        Self {
            engine,
            executions: Some(executions),
            composites: Some(composites),
            connectors: Some(connectors),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_composites(
        engine: FlowEngine,
        composites: Arc<dyn crate::modules::workflow::IWorkflowCompositeExecutionPort>,
    ) -> Self {
        Self {
            engine,
            executions: None,
            composites: Some(composites),
            connectors: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_connectors(
        engine: FlowEngine,
        connectors: Arc<dyn crate::modules::connectors::IWorkflowConnectorPort>,
    ) -> Self {
        Self {
            engine,
            executions: None,
            composites: None,
            connectors: Some(connectors),
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
            let execution_children_terminal = self
                .cancel_execution_children(record, &snapshot, &history, now)
                .await?;
            let composite_children_terminal = self
                .cancel_composite_children(record, &snapshot, &history)
                .await?;
            if !execution_children_terminal || !composite_children_terminal {
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
            self.coordinate_active_composite(record, &snapshot, &history)
                .await?;
            self.coordinate_active_connector(record, &snapshot, &history)
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
            return Err(WorkflowRunCoordinationError::Unavailable(format!(
                "Workflow execution hook {:?} creation history is invalid",
                metadata.flow_hook_id()
            )));
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
mod connector_tests;
#[cfg(test)]
mod tests;
