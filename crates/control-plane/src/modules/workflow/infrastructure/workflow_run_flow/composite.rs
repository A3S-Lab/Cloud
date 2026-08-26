use super::WorkflowLocalStepResult;
use crate::modules::workflow::domain::{
    lookup_workflow_variable_path, materialize_workflow_variables_with_composites,
    ResolvedWorkflowRunStep, WorkflowCompositeFrame, WorkflowCompositeFrameRequest,
    WorkflowCompositeFrameResolution, WorkflowCompositeHookMetadata, WorkflowCompositeRegionPolicy,
    WorkflowCompositeRegionResult, WorkflowCompositeRegionResultRequest,
    WorkflowCompositeResumePayload, WorkflowCompositeWaveHookMetadata,
    WorkflowCompositeWaveRequest, WorkflowIterationRegionPolicy, WorkflowRunInput,
    WorkflowVariableContract, WorkflowVariableDefaults, WORKFLOW_RUN_INPUT_SCHEMA_V22,
    WORKFLOW_RUN_INPUT_SCHEMA_V23, WORKFLOW_RUN_INPUT_SCHEMA_V24,
};
use a3s_flow::{FlowEvent, HookSnapshot, WorkflowContext, WorkflowRunSnapshot};
use chrono::Duration;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) enum CompositeStepResolution {
    AwaitFrame(WorkflowCompositeHookMetadata),
    AwaitWave(WorkflowCompositeWaveHookMetadata),
    Complete(WorkflowCompositeRegionResult),
    Failed(String),
}

pub(super) enum CompositeStepError {
    Invalid(String),
    NonDeterministic(String),
}

pub(super) struct ObservedCompositeHook<'a> {
    pub hook: &'a HookSnapshot,
    pub metadata: WorkflowCompositeHookMetadata,
}

pub(super) fn observed_composite_hooks<'a>(
    input: &WorkflowRunInput,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Vec<ObservedCompositeHook<'a>>, String> {
    let prefixed = snapshot
        .hooks
        .values()
        .filter(|hook| hook.hook_id.starts_with("workflow-composite:"))
        .collect::<Vec<_>>();
    if prefixed.is_empty() {
        return Ok(Vec::new());
    }
    let variables = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| "Workflow composite hook lost its variable contract".to_owned())?
        .restore()?;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| "Workflow composite hook lost its region contract".to_owned())?
        .restore()?;
    let mut observed = Vec::with_capacity(prefixed.len());
    for hook in prefixed {
        let metadata =
            serde_json::from_value::<WorkflowCompositeHookMetadata>(hook.metadata.clone())
                .map_err(|error| format!("Workflow composite hook metadata is invalid: {error}"))?;
        metadata.validate(&input.plan, &regions, &variables)?;
        if matches!(
            input.schema.as_str(),
            WORKFLOW_RUN_INPUT_SCHEMA_V22
                | WORKFLOW_RUN_INPUT_SCHEMA_V23
                | WORKFLOW_RUN_INPUT_SCHEMA_V24
        ) && matches!(
            regions.resolve(&metadata.frame.region_step_id),
            Some(WorkflowCompositeRegionPolicy::Iteration(policy))
                if policy.maximum_concurrency > 1
        ) {
            return Err(
                "Workflow parallel Iteration frame hook is incompatible with a wave-based runtime generation".into(),
            );
        }
        if metadata.frame.organization_id != input.organization_id
            || metadata.frame.project_id != input.project_id
            || metadata.frame.workflow_run_id != input.workflow_run_id
            || metadata.frame.plan_revision_id != input.plan_revision_id
            || metadata.frame.plan_digest != input.plan_digest
            || hook.hook_id != metadata.flow_hook_id()
            || hook.token != metadata.flow_hook_token()
        {
            return Err("Workflow composite hook authority drifted".into());
        }
        observed.push(ObservedCompositeHook { hook, metadata });
    }
    observed.sort_by(|left, right| {
        left.metadata
            .frame
            .region_step_id
            .cmp(&right.metadata.frame.region_step_id)
            .then_with(|| {
                left.metadata
                    .frame
                    .ordinal
                    .cmp(&right.metadata.frame.ordinal)
            })
    });
    let mut previous: Option<(&str, u32)> = None;
    for item in &observed {
        let step_id = item.metadata.frame.region_step_id.as_str();
        let ordinal = item.metadata.frame.ordinal;
        match previous {
            Some((previous_step, previous_ordinal)) if previous_step == step_id => {
                if ordinal
                    != previous_ordinal
                        .checked_add(1)
                        .ok_or_else(|| "Workflow composite hook ordinal overflowed".to_owned())?
                {
                    return Err(
                        "Workflow composite hooks are not contiguous from ordinal zero".into(),
                    );
                }
            }
            _ if ordinal != 0 => {
                return Err("Workflow composite hooks do not start at ordinal zero".into())
            }
            _ => {}
        }
        previous = Some((step_id, ordinal));
    }
    Ok(observed)
}

pub(super) fn resolve_step(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    effective_input: Value,
    outputs: &BTreeMap<String, Value>,
    composites: &BTreeMap<String, WorkflowCompositeRegionResult>,
    context: &WorkflowContext<'_>,
) -> Result<CompositeStepResolution, CompositeStepError> {
    let variables = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| {
            CompositeStepError::Invalid(
                "Workflow composite runtime lost its variable contract".to_owned(),
            )
        })?
        .restore()
        .map_err(CompositeStepError::Invalid)?;
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| resolved.restore())
        .transpose()
        .map_err(CompositeStepError::Invalid)?;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| {
            CompositeStepError::Invalid(
                "Workflow composite runtime lost its region contract".to_owned(),
            )
        })?
        .restore()
        .map_err(CompositeStepError::Invalid)?;
    let policy = regions.resolve(&step.plan.id).ok_or_else(|| {
        CompositeStepError::Invalid("Workflow composite runtime lost its step policy".to_owned())
    })?;
    let available =
        materialize_workflow_variables_with_composites(input, &variables, outputs, composites)
            .map_err(CompositeStepError::Invalid)?;
    let request = region_request(input, &step.plan.id);

    match policy {
        WorkflowCompositeRegionPolicy::Iteration(iteration) => {
            let items = effective_input.as_array().ok_or_else(|| {
                CompositeStepError::Invalid(
                    "Workflow iteration input must be a JSON array of child inputs".to_owned(),
                )
            })?;
            let expected_items = u32::try_from(items.len()).map_err(|_| {
                CompositeStepError::Invalid("Workflow iteration item count overflowed".to_owned())
            })?;
            if expected_items > iteration.maximum_items {
                return Ok(CompositeStepResolution::Failed(
                    "Workflow iteration input exceeds its immutable item bound".into(),
                ));
            }
            if matches!(
                input.schema.as_str(),
                WORKFLOW_RUN_INPUT_SCHEMA_V22
                    | WORKFLOW_RUN_INPUT_SCHEMA_V23
                    | WORKFLOW_RUN_INPUT_SCHEMA_V24
            ) && iteration.maximum_concurrency > 1
            {
                return resolve_parallel_iteration(
                    input,
                    step,
                    items,
                    expected_items,
                    available,
                    request,
                    iteration,
                    &regions,
                    &variables,
                    defaults.as_ref(),
                    context,
                );
            }
            let mut resolutions = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let ordinal = u32::try_from(index).map_err(|_| {
                    CompositeStepError::Invalid(
                        "Workflow iteration frame ordinal overflowed".to_owned(),
                    )
                })?;
                let frame = open_frame(
                    input,
                    &step.plan.id,
                    ordinal,
                    item.clone(),
                    available.clone(),
                    &regions,
                    &variables,
                    defaults.as_ref(),
                )
                .map_err(CompositeStepError::Invalid)?;
                match observe_frame(context, frame, &input.plan, &regions, &variables)? {
                    FrameObservation::Await(hook) => {
                        return Ok(CompositeStepResolution::AwaitFrame(hook))
                    }
                    FrameObservation::Resolved(resolution) => {
                        if let WorkflowCompositeFrameResolution::Failed { error, .. } = &resolution
                        {
                            if iteration.failure_mode
                                == crate::modules::workflow::domain::WorkflowIterationFailureMode::Terminate
                            {
                                return Ok(CompositeStepResolution::Failed(format!(
                                    "Workflow iteration frame {ordinal} failed: {error}"
                                )));
                            }
                        }
                        resolutions.push(resolution);
                    }
                }
            }
            WorkflowCompositeRegionResult::resolve_iteration(
                request,
                expected_items,
                &input.plan,
                &regions,
                &variables,
                resolutions,
            )
            .map(CompositeStepResolution::Complete)
            .map_err(CompositeStepError::Invalid)
        }
        WorkflowCompositeRegionPolicy::Loop(loop_policy) => {
            let mut next_input = effective_input;
            let mut next_available = available;
            let mut resolutions = Vec::new();
            for ordinal in 0..loop_policy.maximum_iterations {
                if loop_time_budget_exhausted(
                    context,
                    &step.plan.id,
                    loop_policy.time_budget_seconds,
                )? {
                    return Ok(CompositeStepResolution::Failed(
                        "Workflow loop exhausted its immutable time budget".into(),
                    ));
                }
                let frame = open_frame(
                    input,
                    &step.plan.id,
                    ordinal,
                    next_input,
                    next_available.clone(),
                    &regions,
                    &variables,
                    defaults.as_ref(),
                )
                .map_err(CompositeStepError::Invalid)?;
                let resolution =
                    match observe_frame(context, frame, &input.plan, &regions, &variables)? {
                        FrameObservation::Await(hook) => {
                            return Ok(CompositeStepResolution::AwaitFrame(hook))
                        }
                        FrameObservation::Resolved(resolution) => resolution,
                    };
                let completed = match &resolution {
                    WorkflowCompositeFrameResolution::Completed { result, .. } => result,
                    WorkflowCompositeFrameResolution::Failed { error, .. } => {
                        return Ok(CompositeStepResolution::Failed(format!(
                            "Workflow loop frame {ordinal} failed: {error}"
                        )));
                    }
                };
                let terminated = lookup_workflow_variable_path(
                    &completed.child_output,
                    &loop_policy.termination_path,
                )
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    CompositeStepError::Invalid(
                        "Workflow loop termination path did not resolve to a boolean".to_owned(),
                    )
                })?;
                next_input = completed.child_output.clone();
                next_available.extend(completed.run_variable_updates.clone());
                resolutions.push(resolution);
                if terminated {
                    return WorkflowCompositeRegionResult::resolve_loop(
                        request,
                        &input.plan,
                        &regions,
                        &variables,
                        resolutions,
                    )
                    .map(CompositeStepResolution::Complete)
                    .map_err(CompositeStepError::Invalid);
                }
            }
            Ok(CompositeStepResolution::Failed(
                "Workflow loop exhausted its immutable maximum iteration count".into(),
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_parallel_iteration(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    items: &[Value],
    expected_items: u32,
    available: BTreeMap<String, Value>,
    request: WorkflowCompositeRegionResultRequest,
    iteration: &WorkflowIterationRegionPolicy,
    regions: &crate::modules::workflow::domain::WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
    defaults: Option<&WorkflowVariableDefaults>,
    context: &WorkflowContext<'_>,
) -> Result<CompositeStepResolution, CompositeStepError> {
    let concurrency = usize::try_from(iteration.maximum_concurrency).map_err(|_| {
        CompositeStepError::Invalid(
            "Workflow iteration concurrency exceeds runtime bounds".to_owned(),
        )
    })?;
    let mut resolutions = Vec::with_capacity(items.len());
    for (wave_index, effective_inputs) in items.chunks(concurrency).enumerate() {
        let first = wave_index.checked_mul(concurrency).ok_or_else(|| {
            CompositeStepError::Invalid("Workflow iteration wave ordinal overflowed".to_owned())
        })?;
        let first_ordinal = u32::try_from(first).map_err(|_| {
            CompositeStepError::Invalid("Workflow iteration wave ordinal overflowed".to_owned())
        })?;
        let metadata = WorkflowCompositeWaveHookMetadata::new(
            WorkflowCompositeWaveRequest {
                organization_id: input.organization_id,
                project_id: input.project_id,
                workflow_run_id: input.workflow_run_id,
                plan_revision_id: input.plan_revision_id,
                plan_digest: input.plan_digest.clone(),
                region_step_id: step.plan.id.clone(),
                first_ordinal,
                effective_inputs: effective_inputs.to_vec(),
                available_variables: available.clone(),
            },
            &input.plan,
            regions,
            variables,
            defaults,
        )
        .map_err(CompositeStepError::Invalid)?;
        let (wave, primary_failure) = match super::composite_wave::observe_wave(
            context, metadata, input, regions, variables, defaults,
        )? {
            super::composite_wave::WaveObservation::Await(metadata) => {
                return Ok(CompositeStepResolution::AwaitWave(metadata))
            }
            super::composite_wave::WaveObservation::Resolved {
                resolutions,
                primary_failure,
            } => (resolutions, primary_failure),
        };
        if iteration.failure_mode
            == crate::modules::workflow::domain::WorkflowIterationFailureMode::Terminate
        {
            if let Some((ordinal, error)) = primary_failure {
                return Ok(CompositeStepResolution::Failed(format!(
                    "Workflow iteration frame {ordinal} failed: {error}"
                )));
            }
        }
        resolutions.extend(wave);
    }
    WorkflowCompositeRegionResult::resolve_iteration(
        request,
        expected_items,
        &input.plan,
        regions,
        variables,
        resolutions,
    )
    .map(CompositeStepResolution::Complete)
    .map_err(CompositeStepError::Invalid)
}

fn loop_time_budget_exhausted(
    context: &WorkflowContext<'_>,
    step_id: &str,
    time_budget_seconds: u64,
) -> Result<bool, CompositeStepError> {
    let prefix = format!("workflow-composite:{step_id}:");
    let Some(started_at) = context
        .history()
        .iter()
        .find_map(|envelope| match &envelope.event {
            FlowEvent::HookCreated { hook_id, .. } if hook_id.starts_with(&prefix) => {
                Some(envelope.timestamp)
            }
            _ => None,
        })
    else {
        return Ok(false);
    };
    let seconds = i64::try_from(time_budget_seconds).map_err(|_| {
        CompositeStepError::Invalid("Workflow loop time budget exceeds runtime bounds".into())
    })?;
    let deadline = started_at
        .checked_add_signed(Duration::seconds(seconds))
        .ok_or_else(|| {
            CompositeStepError::Invalid("Workflow loop time budget overflowed".into())
        })?;
    Ok(context
        .history()
        .last()
        .is_some_and(|event| event.timestamp >= deadline))
}

pub(super) fn validate_result_authority(
    input: &WorkflowRunInput,
    step: &ResolvedWorkflowRunStep,
    result: &WorkflowLocalStepResult,
) -> Result<(), String> {
    result.validate(step)?;
    let Some(region) = result.composite_region_result.as_ref() else {
        return Ok(());
    };
    let variables = input
        .variable_contract
        .as_ref()
        .ok_or_else(|| "Workflow composite result lost its variable contract".to_owned())?
        .restore()?;
    let regions = input
        .composite_regions
        .as_ref()
        .ok_or_else(|| "Workflow composite result lost its region contract".to_owned())?
        .restore()?;
    region.validate(&input.plan, &regions, &variables)
}

#[allow(clippy::too_many_arguments)]
fn open_frame(
    input: &WorkflowRunInput,
    step_id: &str,
    ordinal: u32,
    effective_input: Value,
    available_variables: BTreeMap<String, Value>,
    regions: &crate::modules::workflow::domain::WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
    defaults: Option<&crate::modules::workflow::domain::WorkflowVariableDefaults>,
) -> Result<WorkflowCompositeFrame, String> {
    WorkflowCompositeFrame::open(
        WorkflowCompositeFrameRequest {
            organization_id: input.organization_id,
            project_id: input.project_id,
            workflow_run_id: input.workflow_run_id,
            plan_revision_id: input.plan_revision_id,
            plan_digest: input.plan_digest.clone(),
            region_step_id: step_id.into(),
            ordinal,
            effective_input,
            available_variables,
        },
        &input.plan,
        regions,
        variables,
        defaults,
    )
}

enum FrameObservation {
    Await(WorkflowCompositeHookMetadata),
    Resolved(WorkflowCompositeFrameResolution),
}

fn observe_frame(
    context: &WorkflowContext<'_>,
    frame: WorkflowCompositeFrame,
    plan: &crate::modules::workflow::domain::WorkflowPlan,
    regions: &crate::modules::workflow::domain::WorkflowCompositeRegions,
    variables: &WorkflowVariableContract,
) -> Result<FrameObservation, CompositeStepError> {
    let metadata = WorkflowCompositeHookMetadata::new(frame, plan, regions, variables)
        .map_err(CompositeStepError::Invalid)?;
    let hook_id = metadata.flow_hook_id();
    if context.hook_disposed(&hook_id) {
        return Err(CompositeStepError::NonDeterministic(format!(
            "Workflow composite hook for frame {} was disposed",
            metadata.frame.ordinal
        )));
    }
    let Some(observed) = context.hook_payload(&hook_id) else {
        return Ok(FrameObservation::Await(metadata));
    };
    let payload = serde_json::from_value::<WorkflowCompositeResumePayload>(observed.clone())
        .map_err(|_| CompositeStepError::NonDeterministic(composite_payload_drift(&metadata)))?;
    payload
        .validate(&metadata, plan, regions, variables)
        .map_err(|_| CompositeStepError::NonDeterministic(composite_payload_drift(&metadata)))?;
    Ok(FrameObservation::Resolved(payload.resolution))
}

fn composite_payload_drift(metadata: &WorkflowCompositeHookMetadata) -> String {
    format!(
        "Workflow composite frame {} received an invalid authority-bound payload",
        metadata.frame.ordinal
    )
}

fn region_request(input: &WorkflowRunInput, step_id: &str) -> WorkflowCompositeRegionResultRequest {
    WorkflowCompositeRegionResultRequest {
        organization_id: input.organization_id,
        project_id: input.project_id,
        workflow_run_id: input.workflow_run_id,
        plan_revision_id: input.plan_revision_id,
        plan_digest: input.plan_digest.clone(),
        region_step_id: step_id.into(),
    }
}
