use super::workflow::{
    application_answer_resolution, application_variable_write_resolution,
    connector_failure_route_result, execution_result, human_decision_result, inactive_step_ids,
    local_branch_failure_route_result, local_output_failure_route_result,
    local_transform_failure_route_result, ApplicationAnswerResolution,
    ApplicationVariableWriteResolution, ExecutionResolution,
};
use super::WorkflowLocalStepResult;
use crate::modules::workflow::domain::{
    execution_evidence_references, flow_step_id, WorkflowCompositeFrameResolution,
    WorkflowCompositeRegionPolicy, WorkflowCompositeResumePayload, WorkflowExecutionHookMetadata,
    WorkflowExecutionResumePayload, WorkflowExecutionResumeResolution, WorkflowRunFlowState,
    WorkflowRunInput, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepFailureClassification,
    WorkflowStepFlowState, WorkflowStepKind, WorkflowStepProjectionStatus,
};
use a3s_flow::{
    FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, StepStatus, WorkflowRunSnapshot,
    WorkflowRunStatus as FlowRunStatus, WorkflowTerminalOutcome,
};
use chrono::{DateTime, Utc};
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
        connector_failures,
        application_failures,
        composite_failures,
        workflow_local_failures,
    } = completed_workflow_steps(&record.run.execution_input, &resolved_steps, snapshot)?;
    let inactive = inactive_step_ids(&record.run.execution_input, &completed)?;
    let composite_hooks =
        super::composite::observed_composite_hooks(&record.run.execution_input, snapshot)?;

    for projection in &mut projected.steps {
        let resolved = resolved_steps
            .iter()
            .find(|step| step.plan.id == projection.step_id)
            .ok_or_else(|| format!("WorkflowRun lost resolved step {:?}", projection.step_id))?;
        let durable_step_id = flow_step_id(&projection.step_id);
        let flow_step = snapshot.steps.get(&durable_step_id);
        let variable_hooks = if record
            .run
            .execution_input
            .application_projection
            .as_ref()
            .is_some_and(|application| application.is_variable_assignment_step(&resolved.plan.id))
        {
            let snapshot_hook = application_variable_snapshot_hook(
                &record.run.execution_input,
                resolved,
                snapshot,
            )?;
            snapshot_hook
                .map(|(hook, metadata)| -> Result<_, String> {
                    let application_snapshot =
                        application_variable_snapshot_payload(hook, &metadata)?;
                    let write = application_snapshot
                        .as_ref()
                        .map(|application_snapshot| {
                            application_variable_write_hook(
                                &record.run.execution_input,
                                resolved,
                                application_snapshot,
                                snapshot,
                            )
                        })
                        .transpose()?
                        .flatten();
                    Ok(((hook, metadata), write))
                })
                .transpose()?
        } else {
            None
        };
        let answer_hook = if record
            .run
            .execution_input
            .application_projection
            .as_ref()
            .is_some_and(|application| application.is_answer_step(&resolved.plan.id))
        {
            application_answer_hook(&record.run.execution_input, resolved, snapshot)?
        } else {
            None
        };
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
        let connector_hooks = if resolved.plan.kind == WorkflowStepKind::Service
            && !record
                .run
                .execution_input
                .application_projection
                .as_ref()
                .is_some_and(|application| {
                    application.is_variable_assignment_step(&resolved.plan.id)
                }) {
            super::connector::observed_connector_hooks(
                &record.run.execution_input,
                resolved,
                snapshot,
            )?
        } else {
            Vec::new()
        };
        let connector_evidence_references =
            super::connector::evidence_references(&connector_hooks)?;
        let connector_hook = connector_hooks.last();
        let execution_evidence_references = execution_hook
            .as_ref()
            .map(|(hook, metadata)| projected_execution_evidence_references(hook, metadata))
            .transpose()?
            .unwrap_or_default();
        let composite_hook = (resolved.plan.kind == WorkflowStepKind::Subworkflow)
            .then(|| {
                composite_hooks
                    .iter()
                    .filter(|observed| observed.metadata.frame.region_step_id == resolved.plan.id)
                    .max_by_key(|observed| observed.metadata.frame.ordinal)
            })
            .flatten();
        let (step_status, attempt, result, selected_handle, step_error, sequence, at) =
            if let Some(((snapshot_hook, snapshot_metadata), write_hook)) = variable_hooks {
                let hook = write_hook
                    .as_ref()
                    .map(|(hook, _)| *hook)
                    .unwrap_or(snapshot_hook);
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
                let failure = application_failures.get(&projection.step_id).cloned();
                let step_status = match hook.status {
                    HookStatus::Active | HookStatus::Received if write_hook.is_none() => {
                        WorkflowStepProjectionStatus::Running
                    }
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
                        "Workflow Application variable hook {:?} has unsupported status {status:?}",
                        hook.hook_id
                    ))
                    }
                };
                let completed_result = completed.get(&projection.step_id);
                let result = if step_status == WorkflowStepProjectionStatus::Completed {
                    Some(
                        completed_result
                            .ok_or_else(|| {
                                format!(
                                    "Workflow Application variable step {:?} has no committed result",
                                    projection.step_id
                                )
                            })?
                            .output
                            .clone(),
                    )
                } else {
                    None
                };
                let selected_handle = failure
                    .as_ref()
                    .and(completed_result)
                    .and_then(|result| result.selected_handle.clone());
                (
                    step_status,
                    snapshot_metadata.step_attempt,
                    result,
                    selected_handle,
                    failure,
                    sequence,
                    at,
                )
            } else if let Some((hook, metadata)) = answer_hook {
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
                let failure = application_failures.get(&projection.step_id).cloned();
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
                        "Workflow Application Answer hook {:?} has unsupported status {status:?}",
                        hook.hook_id
                    ))
                    }
                };
                let completed_result = completed.get(&projection.step_id);
                let result = if step_status == WorkflowStepProjectionStatus::Completed {
                    Some(
                        completed_result
                            .ok_or_else(|| {
                                format!(
                                    "Workflow Application Answer step {:?} has no received result",
                                    projection.step_id
                                )
                            })?
                            .output
                            .clone(),
                    )
                } else {
                    None
                };
                let selected_handle = failure
                    .as_ref()
                    .and(completed_result)
                    .and_then(|result| result.selected_handle.clone());
                (
                    step_status,
                    metadata.step_attempt,
                    result,
                    selected_handle,
                    failure,
                    sequence,
                    at,
                )
            } else if let Some((hook, metadata)) = human_hook {
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
            } else if let Some((hook, metadata)) = execution_hook.as_ref() {
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
                let selected_handle = failure
                    .as_ref()
                    .and_then(|_| completed.get(&projection.step_id))
                    .and_then(|result| result.selected_handle.clone());
                (
                    step_status,
                    u32::try_from(metadata.step_attempt).map_err(|_| {
                        "Workflow execution attempt exceeds projection bounds".to_owned()
                    })?,
                    result,
                    selected_handle,
                    failure,
                    sequence,
                    at,
                )
            } else if let Some(observed) = connector_hook {
                let hook = observed.hook;
                let typed_response_sequence =
                    if observed.metadata.requires_typed_response() && flow_step.is_some() {
                        Some(
                            last_step_sequence(history, &durable_step_id).ok_or_else(|| {
                                format!("Flow step {durable_step_id:?} has no history")
                            })?,
                        )
                    } else {
                        None
                    };
                let sequence = if let Some(sequence) = typed_response_sequence {
                    sequence
                } else if hook.status == HookStatus::Cancelled {
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
                let failure = connector_failures.get(&projection.step_id).cloned();
                let completed_result = completed.get(&projection.step_id);
                let step_status = match hook.status {
                    HookStatus::Active => WorkflowStepProjectionStatus::Running,
                    HookStatus::Received if failure.is_some() => {
                        WorkflowStepProjectionStatus::Failed
                    }
                    HookStatus::Received if completed_result.is_some() => {
                        WorkflowStepProjectionStatus::Completed
                    }
                    HookStatus::Received if status.is_terminal() => {
                        if matches!(
                            status,
                            WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut
                        ) {
                            WorkflowStepProjectionStatus::Failed
                        } else {
                            WorkflowStepProjectionStatus::Cancelled
                        }
                    }
                    HookStatus::Received => WorkflowStepProjectionStatus::Running,
                    HookStatus::Disposed | HookStatus::Cancelled => {
                        WorkflowStepProjectionStatus::Cancelled
                    }
                    status => {
                        return Err(format!(
                            "Workflow Connector hook {:?} has unsupported status {status:?}",
                            hook.hook_id
                        ))
                    }
                };
                let result = (step_status == WorkflowStepProjectionStatus::Completed)
                    .then(|| completed_result.map(|result| result.output.clone()))
                    .flatten();
                let selected_handle = failure
                    .as_ref()
                    .and(completed_result)
                    .and_then(|result| result.selected_handle.clone());
                let step_error = if step_status == WorkflowStepProjectionStatus::Failed {
                    failure.or_else(|| snapshot.error.clone())
                } else {
                    None
                };
                (
                    step_status,
                    observed.metadata.step_attempt,
                    result,
                    selected_handle,
                    step_error,
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
                let replay_result = flow_step
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
                        super::composite::validate_result_authority(
                            &record.run.execution_input,
                            resolved,
                            &result,
                        )?;
                        Ok::<WorkflowLocalStepResult, String>(result)
                    })
                    .transpose()?;
                let routed_failure = workflow_local_failures.get(&projection.step_id).cloned();
                let completed_result = completed.get(&projection.step_id);
                let selected_handle =
                    completed_result.and_then(|result| result.selected_handle.clone());
                let output = (status == WorkflowStepProjectionStatus::Completed)
                    .then(|| replay_result.map(|result| result.output))
                    .flatten();
                let step_error = if status == WorkflowStepProjectionStatus::Failed {
                    routed_failure.or_else(|| flow_step.error.clone())
                } else {
                    flow_step.error.clone()
                };
                (
                    status,
                    flow_step.attempt,
                    output,
                    selected_handle,
                    step_error,
                    sequence,
                    at,
                )
            } else if let Some(observed) = composite_hook {
                let hook = observed.hook;
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
                let failure = composite_failures.get(&projection.step_id).cloned();
                let step_status = match hook.status {
                    HookStatus::Active => WorkflowStepProjectionStatus::Running,
                    HookStatus::Received if failure.is_some() => {
                        WorkflowStepProjectionStatus::Failed
                    }
                    HookStatus::Received
                        if matches!(
                            snapshot.status,
                            FlowRunStatus::Failed | FlowRunStatus::Cancelled
                        ) =>
                    {
                        WorkflowStepProjectionStatus::Failed
                    }
                    HookStatus::Received => WorkflowStepProjectionStatus::Running,
                    HookStatus::Disposed | HookStatus::Cancelled => {
                        WorkflowStepProjectionStatus::Cancelled
                    }
                    status => {
                        return Err(format!(
                            "Workflow composite hook {:?} has unsupported status {status:?}",
                            hook.hook_id
                        ))
                    }
                };
                let attempt = observed
                    .metadata
                    .frame
                    .ordinal
                    .checked_add(1)
                    .ok_or_else(|| "Workflow composite attempt overflowed".to_owned())?;
                let step_error = if step_status == WorkflowStepProjectionStatus::Failed {
                    failure.or_else(|| snapshot.error.clone())
                } else {
                    None
                };
                (step_status, attempt, None, None, step_error, sequence, at)
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
                let failed_composite = resolved.plan.kind == WorkflowStepKind::Subworkflow
                    && matches!(
                        status,
                        WorkflowRunStatus::Failed | WorkflowRunStatus::TimedOut
                    );
                (
                    if failed_composite {
                        WorkflowStepProjectionStatus::Failed
                    } else {
                        WorkflowStepProjectionStatus::Cancelled
                    },
                    0,
                    None,
                    None,
                    failed_composite.then(|| {
                        snapshot
                            .error
                            .clone()
                            .unwrap_or_else(|| "Workflow composite step failed".into())
                    }),
                    snapshot.last_sequence,
                    observed_at,
                )
            } else {
                continue;
            };
        let evidence_references = replay_compatible_evidence_references(
            projection.status,
            &projection.evidence_references,
            if resolved.plan.kind == WorkflowStepKind::Execution {
                execution_evidence_references
            } else if resolved.plan.kind == WorkflowStepKind::Service {
                connector_evidence_references
            } else {
                Vec::new()
            },
        );
        let desired = WorkflowStepFlowState {
            status: step_status,
            attempt_generation: attempt,
            selected_handle,
            result,
            error: step_error,
            default_output_evidence: completed
                .get(&projection.step_id)
                .and_then(|result| result.default_output_evidence.clone()),
            evidence_references,
            last_flow_sequence: sequence,
            observed_at: at,
        };
        if projection.status.is_terminal()
            && projection.status == desired.status
            && projection.attempt_generation == desired.attempt_generation
            && projection.selected_handle == desired.selected_handle
            && projection.result == desired.result
            && projection.error == desired.error
            && projection.default_output_evidence == desired.default_output_evidence
            && projection.evidence_references == desired.evidence_references
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

fn projected_execution_evidence_references(
    hook: &HookSnapshot,
    metadata: &WorkflowExecutionHookMetadata,
) -> Result<Vec<String>, String> {
    if hook.status != HookStatus::Received {
        return Ok(Vec::new());
    }
    let payload = hook
        .payload
        .as_ref()
        .ok_or_else(|| format!("Workflow execution hook {:?} has no payload", hook.hook_id))?;
    let payload = serde_json::from_value::<WorkflowExecutionResumePayload>(payload.clone())
        .map_err(|error| format!("Workflow execution resume payload is invalid: {error}"))?;
    payload.validate(metadata)?;
    match &payload.resolution {
        WorkflowExecutionResumeResolution::Completed { output, .. } => {
            execution_evidence_references(output)
        }
        WorkflowExecutionResumeResolution::Rejected { .. } => Ok(Vec::new()),
    }
}

fn replay_compatible_evidence_references(
    persisted_status: WorkflowStepProjectionStatus,
    persisted: &[String],
    projected: Vec<String>,
) -> Vec<String> {
    if persisted_status.is_terminal() && persisted.is_empty() {
        Vec::new()
    } else {
        projected
    }
}

pub(super) struct CompletedWorkflowSteps {
    pub(super) completed: BTreeMap<String, WorkflowLocalStepResult>,
    pub(super) execution_failures: BTreeMap<String, String>,
    pub(super) connector_failures: BTreeMap<String, String>,
    pub(super) application_failures: BTreeMap<String, String>,
    pub(super) composite_failures: BTreeMap<String, String>,
    pub(super) workflow_local_failures: BTreeMap<String, String>,
}

pub(super) fn completed_workflow_steps(
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
        super::composite::validate_result_authority(input, resolved, &result)?;
        if completed.insert(result.step_id.clone(), result).is_some() {
            return Err("WorkflowRun Flow contains duplicate step results".into());
        }
    }

    let mut execution_failures = BTreeMap::new();
    let mut connector_failures = BTreeMap::new();
    let mut application_failures = BTreeMap::new();
    let mut composite_failures = BTreeMap::new();
    let mut workflow_local_failures = BTreeMap::new();
    for resolved in resolved_steps {
        if input
            .application_projection
            .as_ref()
            .is_some_and(|application| application.is_variable_assignment_step(&resolved.plan.id))
        {
            let Some((snapshot_hook, snapshot_metadata)) =
                application_variable_snapshot_hook(input, resolved, snapshot)?
            else {
                continue;
            };
            let Some(application_snapshot) =
                application_variable_snapshot_payload(snapshot_hook, &snapshot_metadata)?
            else {
                continue;
            };
            let Some((write_hook, write_metadata)) =
                application_variable_write_hook(input, resolved, &application_snapshot, snapshot)?
            else {
                continue;
            };
            if write_hook.status == HookStatus::Received {
                let payload = write_hook.payload.as_ref().ok_or_else(|| {
                    format!(
                        "Workflow Application variable write hook {:?} is received without a payload",
                        write_hook.hook_id
                    )
                })?;
                let outputs = completed
                    .iter()
                    .map(|(step_id, result)| (step_id.clone(), result.output.clone()))
                    .collect::<BTreeMap<_, _>>();
                let composites = completed
                    .iter()
                    .filter_map(|(step_id, result)| {
                        result
                            .composite_region_result
                            .clone()
                            .map(|region| (step_id.clone(), region))
                    })
                    .collect::<BTreeMap<_, _>>();
                let values = super::variables::application_assignment_values(
                    input,
                    &resolved.plan.id,
                    &outputs,
                    &composites,
                    &application_snapshot,
                )?;
                let resolution = application_variable_write_resolution(
                    &snapshot.run_id,
                    &write_metadata.flow_hook_id(),
                    input,
                    resolved,
                    &write_metadata,
                    &values,
                    payload,
                )
                .map_err(|error| error.to_string())?;
                match resolution {
                    ApplicationVariableWriteResolution::Completed(result) => {
                        completed.insert(result.step_id.clone(), *result);
                    }
                    ApplicationVariableWriteResolution::Failed { result, message } => {
                        completed.insert(result.step_id.clone(), *result);
                        application_failures.insert(resolved.plan.id.clone(), message);
                    }
                }
            }
            continue;
        }
        if input
            .application_projection
            .as_ref()
            .is_some_and(|application| application.is_answer_step(&resolved.plan.id))
        {
            let Some((hook, metadata)) = application_answer_hook(input, resolved, snapshot)? else {
                continue;
            };
            if hook.status == HookStatus::Received {
                let payload = hook.payload.as_ref().ok_or_else(|| {
                    format!(
                        "Workflow Application Answer hook {:?} is received without a payload",
                        hook.hook_id
                    )
                })?;
                let resolution = application_answer_resolution(
                    &snapshot.run_id,
                    &metadata.flow_hook_id(),
                    input,
                    resolved,
                    &metadata,
                    payload,
                )
                .map_err(|error| error.to_string())?;
                match resolution {
                    ApplicationAnswerResolution::Completed(result) => {
                        completed.insert(result.step_id.clone(), *result);
                    }
                    ApplicationAnswerResolution::Failed { result, message } => {
                        completed.insert(result.step_id.clone(), *result);
                        application_failures.insert(resolved.plan.id.clone(), message);
                    }
                }
            }
            continue;
        }
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
                        input,
                        resolved,
                        &metadata,
                        payload,
                    )
                    .map_err(|error| error.to_string())?
                    {
                        ExecutionResolution::Succeeded(result) => {
                            completed.insert(result.step_id.clone(), *result);
                        }
                        ExecutionResolution::Failed { error, routed } => {
                            if let Some(result) = routed {
                                completed.insert(result.step_id.clone(), *result);
                            }
                            execution_failures.insert(resolved.plan.id.clone(), error);
                        }
                    }
                }
            }
            WorkflowStepKind::Service => {
                let Some(observed) =
                    super::connector::observed_connector_hooks(input, resolved, snapshot)?
                        .into_iter()
                        .last()
                else {
                    continue;
                };
                if observed.hook.status == HookStatus::Received {
                    let failure = match super::connector::project_received_hook(
                        resolved, &observed,
                    )? {
                        super::connector::ConnectorProjectionResolution::Running
                            if observed.metadata.requires_typed_response() =>
                        {
                            let durable_step_id = flow_step_id(&resolved.plan.id);
                            snapshot.steps.get(&durable_step_id).and_then(|step| {
                                (step.status == StepStatus::Failed).then(|| {
                                    super::connector::ConnectorStepFailure {
                                        classification: WorkflowStepFailureClassification::ProviderResponseInvalid,
                                        message: step.error.clone().unwrap_or_else(|| {
                                            "Workflow Connector response step failed without an error"
                                                .into()
                                        }),
                                    }
                                })
                            })
                        }
                        super::connector::ConnectorProjectionResolution::Running => None,
                        super::connector::ConnectorProjectionResolution::Completed(result) => {
                            completed.insert(result.step_id.clone(), *result);
                            None
                        }
                        super::connector::ConnectorProjectionResolution::Failed(failure) => {
                            Some(*failure)
                        }
                    };
                    if let Some(failure) = failure {
                        if let Some(result) = connector_failure_route_result(
                            &snapshot.run_id,
                            input,
                            resolved,
                            failure.classification,
                            failure.message.clone(),
                        )
                        .map_err(|error| error.to_string())?
                        {
                            completed.insert(result.step_id.clone(), *result);
                        }
                        connector_failures.insert(resolved.plan.id.clone(), failure.message);
                    }
                }
            }
            WorkflowStepKind::Transform | WorkflowStepKind::Branch | WorkflowStepKind::Output => {
                let durable_step_id = flow_step_id(&resolved.plan.id);
                if snapshot
                    .steps
                    .get(&durable_step_id)
                    .is_some_and(|step| step.status == StepStatus::Failed)
                {
                    let result = match resolved.plan.kind {
                        WorkflowStepKind::Transform => {
                            local_transform_failure_route_result(&snapshot.run_id, input, resolved)
                        }
                        WorkflowStepKind::Branch => {
                            local_branch_failure_route_result(&snapshot.run_id, input, resolved)
                        }
                        WorkflowStepKind::Output => {
                            local_output_failure_route_result(&snapshot.run_id, input, resolved)
                        }
                        _ => return Err("Workflow local failure projection kind drifted".into()),
                    }
                    .map_err(|error| error.to_string())?;
                    if let Some(result) = result {
                        let failure = serde_json::from_value::<
                            crate::modules::workflow::domain::WorkflowStepFailureOutput,
                        >(result.output.clone())
                        .map_err(|error| {
                            format!("Workflow local failure output is invalid: {error}")
                        })?;
                        workflow_local_failures
                            .insert(resolved.plan.id.clone(), failure.message.clone());
                        completed.insert(result.step_id.clone(), *result);
                    }
                }
            }
            _ => {}
        }
    }
    if input.composite_regions.is_some() {
        let regions = input
            .composite_regions
            .as_ref()
            .ok_or_else(|| "Workflow composite regions disappeared".to_owned())?
            .restore()?;
        let variables = input
            .variable_contract
            .as_ref()
            .ok_or_else(|| "Workflow composite variables disappeared".to_owned())?
            .restore()?;
        for observed in super::composite::observed_composite_hooks(input, snapshot)? {
            if observed.hook.status != HookStatus::Received {
                continue;
            }
            let payload = observed.hook.payload.as_ref().ok_or_else(|| {
                format!(
                    "Workflow composite hook {:?} is received without a payload",
                    observed.hook.hook_id
                )
            })?;
            let payload = serde_json::from_value::<WorkflowCompositeResumePayload>(payload.clone())
                .map_err(|error| {
                    format!("Workflow composite resume payload is invalid: {error}")
                })?;
            payload.validate(&observed.metadata, &input.plan, &regions, &variables)?;
            if let WorkflowCompositeFrameResolution::Failed { error, .. } = payload.resolution {
                let terminal = match regions.resolve(&observed.metadata.frame.region_step_id) {
                    Some(WorkflowCompositeRegionPolicy::Iteration(policy)) => {
                        policy.failure_mode
                            == crate::modules::workflow::domain::WorkflowIterationFailureMode::Terminate
                    }
                    Some(WorkflowCompositeRegionPolicy::Loop(_)) => true,
                    None => {
                        return Err("Workflow composite hook lost its region policy".into())
                    }
                };
                if terminal {
                    composite_failures
                        .insert(observed.metadata.frame.region_step_id.clone(), error);
                }
            }
        }
    }
    Ok(CompletedWorkflowSteps {
        completed,
        execution_failures,
        connector_failures,
        application_failures,
        composite_failures,
        workflow_local_failures,
    })
}

pub(super) use super::projection_authority::{
    application_answer_hook, application_variable_snapshot_hook,
    application_variable_snapshot_payload, application_variable_write_hook, execution_hook,
    human_decision_hook, verify_flow_authority,
};
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

#[cfg(test)]
mod evidence_reference_compatibility_tests {
    use super::*;

    fn references() -> Vec<String> {
        vec!["urn:a3s:cloud:connectors:attempt:019c0000-0000-7000-8000-000000000001".into()]
    }

    #[test]
    fn legacy_terminal_projection_is_not_backfilled() {
        assert!(replay_compatible_evidence_references(
            WorkflowStepProjectionStatus::Completed,
            &[],
            references(),
        )
        .is_empty());
    }

    #[test]
    fn non_terminal_projection_adopts_derived_references() {
        assert_eq!(
            replay_compatible_evidence_references(
                WorkflowStepProjectionStatus::Running,
                &[],
                references(),
            ),
            references()
        );
    }
}
