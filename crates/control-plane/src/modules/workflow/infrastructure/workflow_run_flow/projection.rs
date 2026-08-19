use super::workflow::{
    execution_result, human_decision_result, inactive_step_ids, ExecutionResolution,
};
use super::WorkflowLocalStepResult;
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, inspect_workflow_run_variables, IWorkflowRunHistoryReader,
    IWorkflowRunVariableReader, WorkflowExecutionChildReferenceMetadata,
    WorkflowExecutionHookMetadata, WorkflowExecutionResumePayload,
    WorkflowExecutionResumeResolution, WorkflowHumanDecisionHookMetadata, WorkflowRunFlowState,
    WorkflowRunHistoryEvent, WorkflowRunHistoryPage, WorkflowRunInput, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowRunVariableInspection, WorkflowStepFlowState, WorkflowStepKind,
    WorkflowStepProjectionStatus, WORKFLOW_EXECUTION_STEP_ATTEMPT, WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, RuntimeKind,
    StepStatus, WorkflowRunSnapshot, WorkflowRunStatus as FlowRunStatus, WorkflowTerminalOutcome,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::BTreeMap;

pub fn project_workflow_run_record(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Result<Option<WorkflowRunRecord>, String> {
    verify_flow_authority(record, snapshot, history)?;
    let build_id = snapshot
        .spec
        .runtime_build_id
        .as_ref()
        .ok_or_else(|| "WorkflowRun Flow history is not pinned to a runtime build".to_owned())?
        .as_str()
        .to_owned();
    let started_at = event_time(history, |event| matches!(event, FlowEvent::RunStarted));
    let finished_at = snapshot
        .status
        .is_terminal()
        .then(|| history.last().map(|event| event.timestamp))
        .flatten();
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .ok_or_else(|| "WorkflowRun Flow history is empty".to_owned())?;
    let (status, output, error) = project_run_state(snapshot)?;
    let mut projected = record.clone();
    let expected_version = projected.run.aggregate_version;
    let changed = projected.run.project_flow(WorkflowRunFlowState {
        status,
        flow_runtime_build_id: build_id,
        last_flow_sequence: snapshot.last_sequence,
        output,
        error,
        started_at,
        finished_at,
        observed_at,
    })?;
    if !changed {
        return Ok(None);
    }

    let resolved_steps = record.run.execution_input.resolved_steps()?;
    let CompletedWorkflowSteps {
        completed,
        execution_failures,
    } = completed_workflow_steps(&record.run.execution_input, &resolved_steps, snapshot)?;
    let inactive = inactive_step_ids(&record.run.execution_input, &completed)?;

    for projection in &mut projected.steps {
        let resolved = resolved_steps
            .iter()
            .find(|step| step.plan.id == projection.step_id)
            .ok_or_else(|| format!("WorkflowRun lost resolved step {:?}", projection.step_id))?;
        let durable_step_id = flow_step_id(&projection.step_id);
        let flow_step = snapshot.steps.get(&durable_step_id);
        let human_hook = if resolved.plan.kind == WorkflowStepKind::HumanDecision {
            human_decision_hook(&record.run.execution_input, resolved, snapshot)?
        } else {
            None
        };
        let execution_hook = if resolved.plan.kind == WorkflowStepKind::Execution {
            execution_hook(&record.run.execution_input, resolved, snapshot)?
        } else {
            None
        };
        let (step_status, attempt, result, selected_handle, step_error, sequence, at) =
            if let Some((hook, metadata)) = human_hook {
                let sequence = if hook.status == HookStatus::Cancelled {
                    snapshot.last_sequence
                } else {
                    last_hook_sequence(history, &hook.hook_id)
                        .ok_or_else(|| format!("Flow hook {:?} has no history", hook.hook_id))?
                };
                let at = history
                    .iter()
                    .find(|event| event.sequence == sequence)
                    .map(|event| event.timestamp)
                    .ok_or_else(|| format!("Flow hook {:?} time is missing", hook.hook_id))?;
                let step_status = match hook.status {
                    HookStatus::Active => WorkflowStepProjectionStatus::Running,
                    HookStatus::Received => WorkflowStepProjectionStatus::Completed,
                    HookStatus::Disposed | HookStatus::Cancelled => {
                        WorkflowStepProjectionStatus::Cancelled
                    }
                    status => {
                        return Err(format!(
                            "Workflow human-decision hook {:?} has unsupported status {status:?}",
                            hook.hook_id
                        ))
                    }
                };
                let result = if hook.status == HookStatus::Received {
                    Some(
                        completed
                            .get(&projection.step_id)
                            .ok_or_else(|| {
                                format!(
                                    "Workflow human-decision step {:?} has no received result",
                                    projection.step_id
                                )
                            })?
                            .output
                            .clone(),
                    )
                } else {
                    None
                };
                (
                    step_status,
                    u32::try_from(metadata.step_attempt).map_err(|_| {
                        "Workflow human-decision attempt exceeds projection bounds".to_owned()
                    })?,
                    result,
                    None,
                    None,
                    sequence,
                    at,
                )
            } else if let Some((hook, metadata)) = execution_hook {
                let sequence = if hook.status == HookStatus::Cancelled {
                    snapshot.last_sequence
                } else {
                    last_hook_sequence(history, &hook.hook_id)
                        .ok_or_else(|| format!("Flow hook {:?} has no history", hook.hook_id))?
                };
                let at = history
                    .iter()
                    .find(|event| event.sequence == sequence)
                    .map(|event| event.timestamp)
                    .ok_or_else(|| format!("Flow hook {:?} time is missing", hook.hook_id))?;
                let failure = execution_failures.get(&projection.step_id).cloned();
                let step_status = match hook.status {
                    HookStatus::Active => WorkflowStepProjectionStatus::Running,
                    HookStatus::Received if failure.is_some() => {
                        WorkflowStepProjectionStatus::Failed
                    }
                    HookStatus::Received => WorkflowStepProjectionStatus::Completed,
                    HookStatus::Disposed | HookStatus::Cancelled => {
                        WorkflowStepProjectionStatus::Cancelled
                    }
                    status => {
                        return Err(format!(
                            "Workflow execution hook {:?} has unsupported status {status:?}",
                            hook.hook_id
                        ))
                    }
                };
                let result = if step_status == WorkflowStepProjectionStatus::Completed {
                    Some(
                        completed
                            .get(&projection.step_id)
                            .ok_or_else(|| {
                                format!(
                                    "Workflow execution step {:?} has no received result",
                                    projection.step_id
                                )
                            })?
                            .output
                            .clone(),
                    )
                } else {
                    None
                };
                (
                    step_status,
                    u32::try_from(metadata.step_attempt).map_err(|_| {
                        "Workflow execution attempt exceeds projection bounds".to_owned()
                    })?,
                    result,
                    None,
                    failure,
                    sequence,
                    at,
                )
            } else if let Some(flow_step) = flow_step {
                let sequence = last_step_sequence(history, &durable_step_id)
                    .ok_or_else(|| format!("Flow step {durable_step_id:?} has no history"))?;
                let at = history
                    .iter()
                    .find(|event| event.sequence == sequence)
                    .map(|event| event.timestamp)
                    .ok_or_else(|| format!("Flow step {durable_step_id:?} time is missing"))?;
                let status = match flow_step.status {
                    StepStatus::Pending => WorkflowStepProjectionStatus::Pending,
                    StepStatus::Running => WorkflowStepProjectionStatus::Running,
                    StepStatus::Completed => WorkflowStepProjectionStatus::Completed,
                    StepStatus::Failed => WorkflowStepProjectionStatus::Failed,
                    StepStatus::Cancelled => WorkflowStepProjectionStatus::Cancelled,
                    status => {
                        return Err(format!(
                            "Flow step {durable_step_id:?} has unsupported status {status:?}"
                        ))
                    }
                };
                let result = flow_step
                    .output
                    .as_ref()
                    .map(|value| {
                        let result =
                            serde_json::from_value::<WorkflowLocalStepResult>(value.clone())
                                .map_err(|error| {
                                    format!(
                                        "Flow step {durable_step_id:?} result is invalid: {error}"
                                    )
                                })?;
                        result.validate(resolved)?;
                        Ok::<WorkflowLocalStepResult, String>(result)
                    })
                    .transpose()?;
                let selected_handle = result
                    .as_ref()
                    .and_then(|result| result.selected_handle.clone());
                let output = result.map(|result| result.output);
                (
                    status,
                    flow_step.attempt,
                    output,
                    selected_handle,
                    flow_step.error.clone(),
                    sequence,
                    at,
                )
            } else if inactive.contains(&projection.step_id) {
                (
                    WorkflowStepProjectionStatus::Skipped,
                    0,
                    None,
                    None,
                    None,
                    snapshot.last_sequence,
                    observed_at,
                )
            } else if status.is_terminal() {
                (
                    WorkflowStepProjectionStatus::Cancelled,
                    0,
                    None,
                    None,
                    None,
                    snapshot.last_sequence,
                    observed_at,
                )
            } else {
                continue;
            };
        let desired = WorkflowStepFlowState {
            status: step_status,
            attempt_generation: attempt,
            selected_handle,
            result,
            error: step_error,
            last_flow_sequence: sequence,
            observed_at: at,
        };
        if projection.status.is_terminal()
            && projection.status == desired.status
            && projection.attempt_generation == desired.attempt_generation
            && projection.selected_handle == desired.selected_handle
            && projection.result == desired.result
            && projection.error == desired.error
        {
            continue;
        }
        projection.project_flow(desired)?;
    }
    if projected.run.aggregate_version != expected_version + 1 {
        return Err("WorkflowRun projection did not advance exactly one aggregate version".into());
    }
    projected.validate()?;
    Ok(Some(projected))
}

struct CompletedWorkflowSteps {
    completed: BTreeMap<String, WorkflowLocalStepResult>,
    execution_failures: BTreeMap<String, String>,
}

fn completed_workflow_steps(
    input: &WorkflowRunInput,
    resolved_steps: &[crate::modules::workflow::domain::ResolvedWorkflowRunStep],
    snapshot: &WorkflowRunSnapshot,
) -> Result<CompletedWorkflowSteps, String> {
    let mut completed = BTreeMap::new();
    for (durable_step_id, step) in &snapshot.steps {
        let Some(output) = &step.output else {
            continue;
        };
        let result = serde_json::from_value::<WorkflowLocalStepResult>(output.clone())
            .map_err(|error| format!("WorkflowRun Flow step result is invalid: {error}"))?;
        let resolved = resolved_steps
            .iter()
            .find(|resolved| resolved.plan.id == result.step_id)
            .ok_or_else(|| {
                format!(
                    "WorkflowRun Flow step result {:?} is not declared by the plan",
                    result.step_id
                )
            })?;
        if durable_step_id != &flow_step_id(&result.step_id) {
            return Err("WorkflowRun Flow step result identity drifted".into());
        }
        result.validate(resolved)?;
        if completed.insert(result.step_id.clone(), result).is_some() {
            return Err("WorkflowRun Flow contains duplicate step results".into());
        }
    }

    let mut execution_failures = BTreeMap::new();
    for resolved in resolved_steps {
        match resolved.plan.kind {
            WorkflowStepKind::HumanDecision => {
                let Some((hook, metadata)) = human_decision_hook(input, resolved, snapshot)? else {
                    continue;
                };
                if hook.status == HookStatus::Received {
                    let payload = hook.payload.as_ref().ok_or_else(|| {
                        format!(
                            "Workflow human-decision hook {:?} is received without a payload",
                            hook.hook_id
                        )
                    })?;
                    let result = human_decision_result(
                        &snapshot.run_id,
                        &metadata.flow_hook_id(),
                        resolved,
                        payload,
                    )
                    .map_err(|error| error.to_string())?;
                    completed.insert(result.step_id.clone(), result);
                }
            }
            WorkflowStepKind::Execution => {
                let Some((hook, metadata)) = execution_hook(input, resolved, snapshot)? else {
                    continue;
                };
                if hook.status == HookStatus::Received {
                    let payload = hook.payload.as_ref().ok_or_else(|| {
                        format!(
                            "Workflow execution hook {:?} is received without a payload",
                            hook.hook_id
                        )
                    })?;
                    match execution_result(
                        &snapshot.run_id,
                        &metadata.flow_hook_id(),
                        resolved,
                        &metadata,
                        payload,
                    )
                    .map_err(|error| error.to_string())?
                    {
                        ExecutionResolution::Succeeded(result) => {
                            completed.insert(result.step_id.clone(), result);
                        }
                        ExecutionResolution::Failed(error) => {
                            execution_failures.insert(resolved.plan.id.clone(), error);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(CompletedWorkflowSteps {
        completed,
        execution_failures,
    })
}

pub(super) fn verify_flow_authority(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Result<(), String> {
    if snapshot.run_id != record.run.flow_run_id
        || snapshot.spec.name != record.run.execution_input.flow_workflow_name
        || snapshot.spec.version != record.run.execution_input.flow_workflow_version
        || snapshot.spec.runtime.kind != RuntimeKind::RustEmbedded
        || snapshot.spec.runtime.entrypoint != "a3s-cloud"
        || snapshot.spec.runtime.export_name != "main"
        || snapshot.last_sequence == 0
        || history.last().map(|event| event.sequence) != Some(snapshot.last_sequence)
        || history.iter().any(|event| event.run_id != snapshot.run_id)
    {
        return Err("WorkflowRun correlated Flow identity or history sequence drifted".into());
    }
    let expected_input = serde_json::to_value(&record.run.execution_input)
        .map_err(|error| format!("could not encode WorkflowRun input: {error}"))?;
    if snapshot.input != expected_input {
        return Err("WorkflowRun correlated Flow input drifted".into());
    }
    let resolved_steps = record.run.execution_input.resolved_steps()?;
    let mut expected_hooks = std::collections::BTreeSet::new();
    for resolved in &resolved_steps {
        match resolved.plan.kind {
            WorkflowStepKind::HumanDecision => {
                let expected = WorkflowHumanDecisionHookMetadata::from_run_step(
                    &record.run.execution_input,
                    resolved,
                )?;
                expected_hooks.insert(expected.flow_hook_id());
                human_decision_hook(&record.run.execution_input, resolved, snapshot)?;
            }
            WorkflowStepKind::Execution => {
                let hook_id = format!(
                    "workflow-execution:{}:{}",
                    resolved.plan.id, WORKFLOW_EXECUTION_STEP_ATTEMPT
                );
                expected_hooks.insert(hook_id);
                execution_hook(&record.run.execution_input, resolved, snapshot)?;
            }
            _ => {}
        }
    }
    if snapshot
        .hooks
        .keys()
        .any(|hook_id| !expected_hooks.contains(hook_id))
    {
        return Err("WorkflowRun correlated Flow contains an unexpected hook".into());
    }
    verify_execution_child_references(record, snapshot)?;
    Ok(())
}

fn verify_execution_child_references(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
) -> Result<(), String> {
    let resolved_steps = record.run.execution_input.resolved_steps()?;
    let mut observed = BTreeMap::new();
    for resolved in &resolved_steps {
        if resolved.plan.kind != WorkflowStepKind::Execution {
            continue;
        }
        let Some((hook, metadata)) =
            execution_hook(&record.run.execution_input, resolved, snapshot)?
        else {
            continue;
        };
        observed.insert(metadata.flow_hook_id(), (hook, metadata));
    }
    for (reference_id, child) in &snapshot.child_operations {
        let Some((hook, metadata)) = observed.get(reference_id) else {
            return Err(
                "WorkflowRun correlated Flow contains an unexpected child operation".into(),
            );
        };
        let operation_id = uuid::Uuid::parse_str(&child.operation_id)
            .map_err(|_| "Workflow child Execution operation identity is invalid".to_owned())?;
        if child.reference_id != *reference_id
            || child.kind != "execution"
            || operation_id.is_nil()
            || child.flow_run_id.as_deref() != Some(child.operation_id.as_str())
        {
            return Err("Workflow child Execution reference identity drifted".into());
        }
        let child_metadata = serde_json::from_value::<WorkflowExecutionChildReferenceMetadata>(
            child.metadata.clone(),
        )
        .map_err(|error| format!("Workflow child Execution metadata is invalid: {error}"))?;
        child_metadata.validate(metadata)?;
        if hook.status != HookStatus::Received {
            continue;
        }
        let payload = hook
            .payload
            .as_ref()
            .ok_or_else(|| "received Workflow execution hook has no payload".to_owned())?;
        let payload = serde_json::from_value::<WorkflowExecutionResumePayload>(payload.clone())
            .map_err(|error| format!("Workflow execution resume payload is invalid: {error}"))?;
        payload.validate(metadata)?;
        match payload.resolution {
            WorkflowExecutionResumeResolution::Completed { output, .. }
                if output.execution_id.as_uuid() == operation_id
                    && output.operation_id.as_uuid() == operation_id
                    && output.invocation_template_digest
                        == child_metadata.invocation_template_digest => {}
            WorkflowExecutionResumeResolution::Completed { .. } => {
                return Err(
                    "Workflow execution result changed its child operation authority".into(),
                )
            }
            WorkflowExecutionResumeResolution::Rejected { .. } => {
                return Err(
                    "rejected Workflow execution dispatch unexpectedly linked a child".into(),
                )
            }
        }
    }
    for (reference_id, (hook, metadata)) in observed {
        if hook.status != HookStatus::Received {
            continue;
        }
        let payload = hook
            .payload
            .as_ref()
            .ok_or_else(|| "received Workflow execution hook has no payload".to_owned())?;
        let payload = serde_json::from_value::<WorkflowExecutionResumePayload>(payload.clone())
            .map_err(|error| format!("Workflow execution resume payload is invalid: {error}"))?;
        payload.validate(&metadata)?;
        let linked = snapshot.child_operations.contains_key(&reference_id);
        match payload.resolution {
            WorkflowExecutionResumeResolution::Completed { .. } if !linked => {
                return Err(
                    "completed Workflow execution result has no durable child reference".into(),
                )
            }
            WorkflowExecutionResumeResolution::Rejected { .. } if linked => {
                return Err(
                    "rejected Workflow execution dispatch unexpectedly linked a child".into(),
                )
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn execution_hook<'a>(
    input: &WorkflowRunInput,
    resolved: &crate::modules::workflow::domain::ResolvedWorkflowRunStep,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Option<(&'a HookSnapshot, WorkflowExecutionHookMetadata)>, String> {
    let hook_id = format!(
        "workflow-execution:{}:{}",
        resolved.plan.id, WORKFLOW_EXECUTION_STEP_ATTEMPT
    );
    let Some(hook) = snapshot.hooks.get(&hook_id) else {
        return Ok(None);
    };
    let observed =
        serde_json::from_value::<WorkflowExecutionHookMetadata>(hook.metadata.clone())
            .map_err(|error| format!("Workflow execution hook metadata is invalid: {error}"))?;
    observed.validate()?;
    let expected = WorkflowExecutionHookMetadata::from_run_step(
        input,
        resolved,
        observed.effective_input.clone(),
    )?;
    if hook.hook_id != hook_id || hook.token != expected.flow_hook_token() || observed != expected {
        return Err("Workflow execution hook authority drifted".into());
    }
    Ok(Some((hook, observed)))
}

fn human_decision_hook<'a>(
    input: &WorkflowRunInput,
    resolved: &crate::modules::workflow::domain::ResolvedWorkflowRunStep,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Option<(&'a HookSnapshot, WorkflowHumanDecisionHookMetadata)>, String> {
    let expected = WorkflowHumanDecisionHookMetadata::from_run_step(input, resolved)?;
    let hook_id = expected.flow_hook_id();
    let Some(hook) = snapshot.hooks.get(&hook_id) else {
        return Ok(None);
    };
    let observed =
        serde_json::from_value::<WorkflowHumanDecisionHookMetadata>(hook.metadata.clone())
            .map_err(|error| {
                format!("Workflow human-decision hook metadata is invalid: {error}")
            })?;
    observed.validate()?;
    if hook.hook_id != hook_id || hook.token != expected.flow_hook_token() || observed != expected {
        return Err("Workflow human-decision hook authority drifted".into());
    }
    Ok(Some((hook, observed)))
}

fn project_run_state(
    snapshot: &WorkflowRunSnapshot,
) -> Result<(WorkflowRunStatus, Option<serde_json::Value>, Option<String>), String> {
    match snapshot.status {
        FlowRunStatus::Pending => Ok((WorkflowRunStatus::Pending, None, None)),
        FlowRunStatus::Running => Ok((WorkflowRunStatus::Running, None, None)),
        FlowRunStatus::Suspended => Ok((WorkflowRunStatus::Waiting, None, None)),
        FlowRunStatus::Cancelling => Ok((WorkflowRunStatus::Cancelling, None, None)),
        FlowRunStatus::Completed => Ok((
            WorkflowRunStatus::Completed,
            Some(
                snapshot
                    .output
                    .clone()
                    .ok_or_else(|| "completed Flow run is missing output".to_owned())?,
            ),
            None,
        )),
        FlowRunStatus::Cancelled => Ok((WorkflowRunStatus::Cancelled, None, None)),
        FlowRunStatus::Failed => match &snapshot.terminal_outcome {
            Some(WorkflowTerminalOutcome::TimedOut { reason, deadline }) => Ok((
                WorkflowRunStatus::TimedOut,
                None,
                Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| format!("WorkflowRun timed out at {deadline}")),
                ),
            )),
            _ => Ok((
                WorkflowRunStatus::Failed,
                None,
                Some(
                    snapshot
                        .error
                        .clone()
                        .unwrap_or_else(|| "WorkflowRun failed without a Flow error".into()),
                ),
            )),
        },
        status => Err(format!(
            "Flow run {:?} has unsupported status {status:?}",
            snapshot.run_id
        )),
    }
}

fn event_time(
    history: &[FlowEventEnvelope],
    predicate: impl Fn(&FlowEvent) -> bool,
) -> Option<DateTime<Utc>> {
    history
        .iter()
        .find(|event| predicate(&event.event))
        .map(|event| event.timestamp)
}

fn last_step_sequence(history: &[FlowEventEnvelope], expected_step_id: &str) -> Option<u64> {
    history.iter().rev().find_map(|envelope| {
        let step_id = match &envelope.event {
            FlowEvent::StepCreated { step_id, .. }
            | FlowEvent::StepStarted { step_id, .. }
            | FlowEvent::StepCompleted { step_id, .. }
            | FlowEvent::StepRetrying { step_id, .. }
            | FlowEvent::StepFailed { step_id, .. }
            | FlowEvent::RunRetryExhausted { step_id, .. } => step_id,
            _ => return None,
        };
        (step_id == expected_step_id).then_some(envelope.sequence)
    })
}

fn last_hook_sequence(history: &[FlowEventEnvelope], expected_hook_id: &str) -> Option<u64> {
    history.iter().rev().find_map(|envelope| {
        let hook_id = match &envelope.event {
            FlowEvent::HookCreated { hook_id, .. }
            | FlowEvent::HookReceived { hook_id, .. }
            | FlowEvent::HookDisposed { hook_id } => hook_id,
            _ => return None,
        };
        (hook_id == expected_hook_id).then_some(envelope.sequence)
    })
}

#[derive(Clone)]
pub struct WorkflowRunVariableReader {
    engine: FlowEngine,
}

impl WorkflowRunVariableReader {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }

    async fn inspect_record(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunVariableInspection, FlowError> {
        for attempt in 0..3 {
            let snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
                Ok(snapshot) => snapshot,
                Err(FlowError::RunNotFound(_)) if record.run.last_flow_sequence == 0 => {
                    return inspect_workflow_run_variables(
                        record,
                        0,
                        record.run.requested_at,
                        &BTreeMap::new(),
                    )
                    .map_err(FlowError::Runtime)
                }
                Err(error) => return Err(error),
            };
            let history = self.engine.history(&record.run.flow_run_id).await?;
            if history.last().map(|event| event.sequence) != Some(snapshot.last_sequence) {
                if attempt < 2 {
                    tokio::task::yield_now().await;
                    continue;
                }
                return Err(FlowError::Runtime(
                    "Workflow variable inspection observed concurrent Flow transitions".into(),
                ));
            }
            verify_flow_authority(record, &snapshot, &history).map_err(FlowError::Runtime)?;
            if snapshot.last_sequence < record.run.last_flow_sequence {
                return Err(FlowError::Runtime(
                    "Workflow variable inspection precedes the persisted Flow projection".into(),
                ));
            }
            let resolved_steps = record
                .run
                .execution_input
                .resolved_steps()
                .map_err(FlowError::Runtime)?;
            let completed =
                completed_workflow_steps(&record.run.execution_input, &resolved_steps, &snapshot)
                    .map_err(FlowError::Runtime)?
                    .completed;
            let outputs = completed
                .into_iter()
                .map(|(step_id, result)| (step_id, result.output))
                .collect::<BTreeMap<_, _>>();
            let observed_at = history
                .last()
                .map(|event| event.timestamp)
                .ok_or_else(|| FlowError::Runtime("WorkflowRun Flow history is empty".into()))?;
            return inspect_workflow_run_variables(
                record,
                snapshot.last_sequence,
                observed_at,
                &outputs,
            )
            .map_err(FlowError::Runtime);
        }
        Err(FlowError::Runtime(
            "Workflow variable inspection exhausted its observation attempts".into(),
        ))
    }
}

#[async_trait::async_trait]
impl IWorkflowRunVariableReader for WorkflowRunVariableReader {
    async fn inspect(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunVariableInspection, String> {
        self.inspect_record(record)
            .await
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone)]
pub struct WorkflowRunHistoryReader {
    engine: FlowEngine,
}

impl WorkflowRunHistoryReader {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }

    async fn read_page(
        &self,
        flow_run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<WorkflowRunHistoryPage, FlowError> {
        let limit = limit.clamp(1, 100);
        let history = match self.engine.history(flow_run_id).await {
            Ok(history) => history,
            Err(FlowError::RunNotFound(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut selected = history
            .into_iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit + 1)
            .collect::<Vec<_>>();
        let has_more = selected.len() > limit;
        if has_more {
            selected.pop();
        }
        let events = selected
            .iter()
            .map(summarize_event)
            .collect::<Result<Vec<_>, _>>()
            .map_err(FlowError::Serialization)?;
        let next_sequence = has_more
            .then(|| events.last().map(|event| event.sequence))
            .flatten();
        Ok(WorkflowRunHistoryPage {
            events,
            next_sequence,
        })
    }
}

#[async_trait::async_trait]
impl IWorkflowRunHistoryReader for WorkflowRunHistoryReader {
    async fn read(
        &self,
        flow_run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<WorkflowRunHistoryPage, String> {
        self.read_page(flow_run_id, after_sequence, limit)
            .await
            .map_err(|error| error.to_string())
    }
}

fn summarize_event(
    envelope: &FlowEventEnvelope,
) -> Result<WorkflowRunHistoryEvent, serde_json::Error> {
    let (step_id, attempt, details) = match &envelope.event {
        FlowEvent::RunCreated { spec, input } => {
            let run_input = serde_json::from_value::<WorkflowRunInput>(input.clone()).ok();
            (
                None,
                None,
                json!({
                    "workflowName": spec.name,
                    "workflowVersion": spec.version,
                    "runtimeBuildId": spec.runtime_build_id,
                    "workflowRunId": run_input.as_ref().map(|value| value.workflow_run_id),
                    "planRevisionId": run_input.as_ref().map(|value| value.plan_revision_id),
                    "planDigest": run_input.as_ref().map(|value| &value.plan_digest),
                    "inputDigest": run_input.as_ref().map(|value| &value.plan.input_digest),
                }),
            )
        }
        FlowEvent::RunStarted => (None, None, json!({})),
        FlowEvent::RunCompleted { output } => (None, None, json!({"output": output})),
        FlowEvent::RunFailed { error } => (None, None, json!({"error": error})),
        FlowEvent::RunCancellationRequested { request } => {
            (None, None, json!({"reason": request.reason}))
        }
        FlowEvent::RunCancelled { reason } => (None, None, json!({"reason": reason})),
        FlowEvent::RunTimedOut { deadline, reason } => {
            (None, None, json!({"deadline": deadline, "reason": reason}))
        }
        FlowEvent::RunRetryExhausted {
            step_id,
            attempt,
            error,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error}),
        ),
        FlowEvent::RunHostShutdown { reason } => (None, None, json!({"reason": reason})),
        FlowEvent::RunContinuedAsNew {
            successor_run_id, ..
        } => (
            None,
            None,
            json!({
                "successorRunId": successor_run_id,
                "input": "redacted",
            }),
        ),
        FlowEvent::RunProgressRecorded { progress } => {
            (None, None, serde_json::to_value(progress)?)
        }
        FlowEvent::ChildOperationLinked { child } => (None, None, serde_json::to_value(child)?),
        FlowEvent::ChildWorkflowRequested {
            child_id,
            child_run_id,
            spec,
            cancellation_policy,
            ..
        } => (
            None,
            None,
            json!({
                "childId": child_id,
                "childRunId": child_run_id,
                "workflowName": spec.name,
                "workflowVersion": spec.version,
                "runtimeBuildId": spec.runtime_build_id,
                "cancellationPolicy": cancellation_policy,
                "input": "redacted",
            }),
        ),
        FlowEvent::ChildWorkflowResolved { child_id, outcome } => {
            (None, None, json!({"childId": child_id, "outcome": outcome}))
        }
        FlowEvent::SignalReceived { signal } => (
            None,
            None,
            json!({
                "signalId": signal.signal_id,
                "name": signal.name,
                "payload": "redacted",
            }),
        ),
        FlowEvent::SignalWaitCreated {
            wait_id,
            signal_name,
        } => (
            None,
            None,
            json!({"waitId": wait_id, "signalName": signal_name}),
        ),
        FlowEvent::SignalWaitCompleted { wait_id, signal_id } => (
            None,
            None,
            json!({"waitId": wait_id, "signalId": signal_id}),
        ),
        FlowEvent::StepCreated {
            step_id,
            step_name,
            input,
            retry,
        } => {
            let canonical = canonical_json_bounded(
                input,
                WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                "Workflow history step input",
            )
            .unwrap_or_default();
            (
                Some(step_id.clone()),
                None,
                json!({
                    "stepName": step_name,
                    "inputDigest": Sha256Digest::parse(sha256_digest(&canonical)).ok(),
                    "retry": retry,
                }),
            )
        }
        FlowEvent::StepStarted { step_id, attempt } => {
            (Some(step_id.clone()), Some(*attempt), json!({}))
        }
        FlowEvent::StepCompleted { step_id, output } => {
            (Some(step_id.clone()), None, json!({"result": output}))
        }
        FlowEvent::StepRetrying {
            step_id,
            attempt,
            error,
            retry_after,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error, "retryAfter": retry_after}),
        ),
        FlowEvent::StepFailed {
            step_id,
            attempt,
            error,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error}),
        ),
        FlowEvent::WaitCreated { wait_id, resume_at } => (
            None,
            None,
            json!({"waitId": wait_id, "resumeAt": resume_at}),
        ),
        FlowEvent::WaitCompleted { wait_id } => (None, None, json!({"waitId": wait_id})),
        FlowEvent::HookCreated {
            hook_id, metadata, ..
        } => (None, None, json!({"hookId": hook_id, "metadata": metadata})),
        FlowEvent::HookReceived { hook_id, .. } => (
            None,
            None,
            json!({"hookId": hook_id, "payload": "redacted"}),
        ),
        FlowEvent::HookDisposed { hook_id } => (None, None, json!({"hookId": hook_id})),
        event => (
            None,
            None,
            json!({
                "eventKey": event.event_key(),
                "projection": "unsupported"
            }),
        ),
    };
    Ok(WorkflowRunHistoryEvent {
        sequence: envelope.sequence,
        event_id: envelope.event_id,
        event_key: envelope.event.event_key().into(),
        occurred_at: envelope.timestamp,
        step_id,
        attempt,
        details,
    })
}

#[cfg(test)]
mod history_summary_tests {
    use super::*;
    use a3s_flow::WorkflowSignal;
    use uuid::Uuid;

    fn envelope(event: FlowEvent) -> FlowEventEnvelope {
        FlowEventEnvelope::new("run-1", 1, Uuid::now_v7(), Utc::now(), event)
    }

    #[test]
    fn signal_history_preserves_identity_without_exposing_payload() {
        let summary = summarize_event(&envelope(FlowEvent::SignalReceived {
            signal: WorkflowSignal::new(
                "signal-1",
                "approval.received",
                json!({"secret": "must-not-leak"}),
            ),
        }))
        .expect("signal history summary");

        assert_eq!(summary.event_key, "flow.signal.received");
        assert_eq!(summary.details["signalId"], "signal-1");
        assert_eq!(summary.details["name"], "approval.received");
        assert_eq!(summary.details["payload"], "redacted");
        assert!(!summary.details.to_string().contains("must-not-leak"));
    }

    #[test]
    fn continuation_history_preserves_successor_without_exposing_input() {
        let summary = summarize_event(&envelope(FlowEvent::RunContinuedAsNew {
            successor_run_id: "run-2".into(),
            input: json!({"secret": "must-not-leak"}),
        }))
        .expect("continuation history summary");

        assert_eq!(summary.event_key, "flow.run.continued_as_new");
        assert_eq!(summary.details["successorRunId"], "run-2");
        assert_eq!(summary.details["input"], "redacted");
        assert!(!summary.details.to_string().contains("must-not-leak"));
    }
}
