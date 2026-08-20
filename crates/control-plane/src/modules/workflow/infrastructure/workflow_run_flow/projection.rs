use super::workflow::{
    execution_result, human_decision_result, inactive_step_ids, ExecutionResolution,
};
use super::WorkflowLocalStepResult;
use crate::modules::workflow::domain::{
    flow_step_id, WorkflowCompositeChildReferenceMetadata, WorkflowCompositeFrameResolution,
    WorkflowCompositeRegionPolicy, WorkflowCompositeResumePayload,
    WorkflowExecutionChildReferenceMetadata, WorkflowExecutionHookMetadata,
    WorkflowExecutionResumePayload, WorkflowExecutionResumeResolution,
    WorkflowHumanDecisionHookMetadata, WorkflowRunFlowState, WorkflowRunInput, WorkflowRunRecord,
    WorkflowRunStatus, WorkflowStepFlowState, WorkflowStepKind, WorkflowStepProjectionStatus,
    WORKFLOW_EXECUTION_STEP_ATTEMPT,
};
use a3s_flow::{
    FlowEvent, FlowEventEnvelope, HookSnapshot, HookStatus, RuntimeKind, StepStatus,
    WorkflowRunSnapshot, WorkflowRunStatus as FlowRunStatus, WorkflowTerminalOutcome,
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
        composite_failures,
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
        let connector_hook = if resolved.plan.kind == WorkflowStepKind::Service {
            super::connector::observed_connector_hooks(
                &record.run.execution_input,
                resolved,
                snapshot,
            )?
            .into_iter()
            .last()
        } else {
            None
        };
        let composite_hook = (resolved.plan.kind == WorkflowStepKind::Subworkflow)
            .then(|| {
                composite_hooks
                    .iter()
                    .filter(|observed| observed.metadata.frame.region_step_id == resolved.plan.id)
                    .max_by_key(|observed| observed.metadata.frame.ordinal)
            })
            .flatten();
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
                let step_error = if step_status == WorkflowStepProjectionStatus::Failed {
                    failure.or_else(|| snapshot.error.clone())
                } else {
                    None
                };
                (
                    step_status,
                    observed.metadata.step_attempt,
                    result,
                    None,
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
                        super::composite::validate_result_authority(
                            &record.run.execution_input,
                            resolved,
                            &result,
                        )?;
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
        let desired = WorkflowStepFlowState {
            status: step_status,
            attempt_generation: attempt,
            selected_handle,
            result,
            error: step_error,
            default_output_evidence: completed
                .get(&projection.step_id)
                .and_then(|result| result.default_output_evidence.clone()),
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

pub(super) struct CompletedWorkflowSteps {
    pub(super) completed: BTreeMap<String, WorkflowLocalStepResult>,
    pub(super) execution_failures: BTreeMap<String, String>,
    pub(super) connector_failures: BTreeMap<String, String>,
    pub(super) composite_failures: BTreeMap<String, String>,
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
    let mut composite_failures = BTreeMap::new();
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
                    match super::connector::project_received_hook(resolved, &observed)? {
                        super::connector::ConnectorProjectionResolution::Running => {}
                        super::connector::ConnectorProjectionResolution::Completed(result) => {
                            completed.insert(result.step_id.clone(), *result);
                        }
                        super::connector::ConnectorProjectionResolution::Failed(error) => {
                            connector_failures.insert(resolved.plan.id.clone(), error);
                        }
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
        composite_failures,
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
            WorkflowStepKind::Service => {
                let observed = super::connector::observed_connector_hooks(
                    &record.run.execution_input,
                    resolved,
                    snapshot,
                )?;
                super::connector::verify_hook_history(&observed, history)?;
                super::connector::verify_wait_authority(
                    &record.run.execution_input,
                    snapshot,
                    &observed,
                )?;
                for hook in observed {
                    expected_hooks.insert(hook.metadata.flow_hook_id());
                }
            }
            _ => {}
        }
    }
    for observed in
        super::composite::observed_composite_hooks(&record.run.execution_input, snapshot)?
    {
        expected_hooks.insert(observed.metadata.flow_hook_id());
    }
    if snapshot
        .hooks
        .keys()
        .any(|hook_id| !expected_hooks.contains(hook_id))
    {
        return Err("WorkflowRun correlated Flow contains an unexpected hook".into());
    }
    verify_execution_child_references(record, snapshot)?;
    verify_composite_child_references(record, snapshot)?;
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
        if child.kind == "workflow_run" {
            continue;
        }
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

fn verify_composite_child_references(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
) -> Result<(), String> {
    let input = &record.run.execution_input;
    let variables = input
        .variable_contract
        .as_ref()
        .map(|contract| contract.restore())
        .transpose()?;
    let regions = input
        .composite_regions
        .as_ref()
        .map(|contract| contract.restore())
        .transpose()?;
    let observed = super::composite::observed_composite_hooks(input, snapshot)?
        .into_iter()
        .map(|observed| (observed.metadata.flow_hook_id(), observed))
        .collect::<BTreeMap<_, _>>();
    for (reference_id, child) in &snapshot.child_operations {
        if child.kind != "workflow_run" {
            continue;
        }
        let Some(observed) = observed.get(reference_id) else {
            return Err(
                "WorkflowRun correlated Flow contains an unexpected composite child".into(),
            );
        };
        let operation_id = uuid::Uuid::parse_str(&child.operation_id)
            .map_err(|_| "Workflow composite child operation identity is invalid".to_owned())?;
        let child_metadata = serde_json::from_value::<WorkflowCompositeChildReferenceMetadata>(
            child.metadata.clone(),
        )
        .map_err(|error| format!("Workflow composite child metadata is invalid: {error}"))?;
        child_metadata.validate(&observed.metadata)?;
        if child.reference_id != *reference_id
            || operation_id != child_metadata.child_operation_id.as_uuid()
            || child.flow_run_id.as_deref() != Some(child.operation_id.as_str())
        {
            return Err("Workflow composite child reference identity drifted".into());
        }
    }
    let (Some(variables), Some(regions)) = (variables.as_ref(), regions.as_ref()) else {
        if observed.is_empty() {
            return Ok(());
        }
        return Err("Workflow composite child lost its immutable contracts".into());
    };
    for (reference_id, observed) in observed {
        if observed.hook.status != HookStatus::Received {
            continue;
        }
        let payload = observed
            .hook
            .payload
            .as_ref()
            .ok_or_else(|| "received Workflow composite hook has no payload".to_owned())?;
        let payload = serde_json::from_value::<WorkflowCompositeResumePayload>(payload.clone())
            .map_err(|error| format!("Workflow composite resume payload is invalid: {error}"))?;
        payload.validate(&observed.metadata, &input.plan, regions, variables)?;
        if matches!(
            payload.resolution,
            WorkflowCompositeFrameResolution::Completed { .. }
        ) && !snapshot.child_operations.contains_key(&reference_id)
        {
            return Err("completed Workflow composite frame has no durable child reference".into());
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
