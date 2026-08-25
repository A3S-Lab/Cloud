use crate::modules::workflow::domain::{
    WorkflowApplicationAnswerHookMetadata, WorkflowApplicationVariableSnapshotHookMetadata,
    WorkflowApplicationVariableSnapshotResumePayload, WorkflowApplicationVariableWriteHookMetadata,
    WorkflowCompositeChildReferenceMetadata, WorkflowCompositeFrameResolution,
    WorkflowCompositeResumePayload, WorkflowCompositeWaveResumePayload,
    WorkflowExecutionChildReferenceMetadata, WorkflowExecutionHookMetadata,
    WorkflowExecutionResumePayload, WorkflowExecutionResumeResolution,
    WorkflowHumanDecisionHookMetadata, WorkflowRunInput, WorkflowRunRecord, WorkflowStepKind,
    WORKFLOW_EXECUTION_STEP_ATTEMPT,
};
use a3s_flow::{FlowEventEnvelope, HookSnapshot, HookStatus, RuntimeKind, WorkflowRunSnapshot};
use std::collections::BTreeMap;
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
        let application = record.run.execution_input.application_projection.as_ref();
        if application.is_some_and(|projection| projection.is_variable_step(&resolved.plan.id)) {
            let snapshot_metadata = WorkflowApplicationVariableSnapshotHookMetadata::from_run_step(
                &record.run.execution_input,
                resolved,
            )?;
            expected_hooks.insert(snapshot_metadata.flow_hook_id());
            let snapshot_hook = application_variable_snapshot_hook(
                &record.run.execution_input,
                resolved,
                snapshot,
            )?;
            if application
                .is_some_and(|projection| projection.is_variable_assignment_step(&resolved.plan.id))
            {
                let write_hook_id = format!(
                    "workflow-application-variable-write:{}:{}",
                    resolved.plan.id,
                    crate::modules::workflow::domain::WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT
                );
                expected_hooks.insert(write_hook_id.clone());
                let application_snapshot = snapshot_hook
                    .as_ref()
                    .map(|(hook, metadata)| application_variable_snapshot_payload(hook, metadata))
                    .transpose()?
                    .flatten();
                match application_snapshot.as_ref() {
                    Some(application_snapshot) => {
                        application_variable_write_hook(
                            &record.run.execution_input,
                            resolved,
                            application_snapshot,
                            snapshot,
                        )?;
                    }
                    None if snapshot.hooks.contains_key(&write_hook_id) => return Err(
                        "Workflow Application variable write hook precedes its snapshot evidence"
                            .into(),
                    ),
                    None => {}
                }
                continue;
            }
        }
        if application.is_some_and(|projection| projection.is_answer_step(&resolved.plan.id)) {
            let hook_id = format!(
                "workflow-application-answer:{}:{}",
                resolved.plan.id,
                crate::modules::workflow::domain::WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT
            );
            expected_hooks.insert(hook_id);
            application_answer_hook(&record.run.execution_input, resolved, snapshot)?;
            continue;
        }
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
                super::connector_response::verify_step_history(
                    &record.run.execution_input,
                    resolved,
                    &observed,
                    snapshot,
                    history,
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
    for observed in
        super::composite_wave::observed_composite_wave_hooks(&record.run.execution_input, snapshot)?
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
    let observed_frames = super::composite_wave::observed_composite_frames(input, snapshot)?;
    let by_reference = observed_frames
        .iter()
        .map(|observed| (observed.child_reference_id.clone(), observed))
        .collect::<BTreeMap<_, _>>();
    for (reference_id, child) in &snapshot.child_operations {
        if child.kind != "workflow_run" {
            continue;
        }
        let Some(observed) = by_reference.get(reference_id) else {
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
        child_metadata.validate_frame(&observed.frame)?;
        if child.reference_id != *reference_id
            || child.reference_id != observed.frame.child_reference_id()
            || operation_id != child_metadata.child_operation_id.as_uuid()
            || child.flow_run_id.as_deref() != Some(child.operation_id.as_str())
        {
            return Err("Workflow composite child reference identity drifted".into());
        }
    }
    let (Some(variables), Some(regions)) = (variables.as_ref(), regions.as_ref()) else {
        if observed_frames.is_empty() {
            return Ok(());
        }
        return Err("Workflow composite child lost its immutable contracts".into());
    };
    for observed in super::composite::observed_composite_hooks(input, snapshot)? {
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
        ) && !snapshot
            .child_operations
            .contains_key(&observed.metadata.frame.child_reference_id())
        {
            return Err("completed Workflow composite frame has no durable child reference".into());
        }
    }
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| resolved.restore())
        .transpose()?;
    for observed in super::composite_wave::observed_composite_wave_hooks(input, snapshot)? {
        if observed.hook.status != HookStatus::Received {
            continue;
        }
        let payload = observed
            .hook
            .payload
            .as_ref()
            .ok_or_else(|| "received Workflow composite wave has no payload".to_owned())?;
        let payload = serde_json::from_value::<WorkflowCompositeWaveResumePayload>(payload.clone())
            .map_err(|error| {
                format!("Workflow composite wave resume payload is invalid: {error}")
            })?;
        let resolutions = payload.frame_resolutions(
            &observed.metadata,
            &input.plan,
            regions,
            variables,
            defaults.as_ref(),
        )?;
        for resolution in resolutions {
            if matches!(
                resolution,
                WorkflowCompositeFrameResolution::Completed { .. }
            ) && !snapshot
                .child_operations
                .contains_key(&resolution.frame().child_reference_id())
            {
                return Err(
                    "completed Workflow composite wave frame has no durable child reference".into(),
                );
            }
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

pub(super) fn application_variable_snapshot_hook<'a>(
    input: &WorkflowRunInput,
    resolved: &crate::modules::workflow::domain::ResolvedWorkflowRunStep,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<
    Option<(
        &'a HookSnapshot,
        WorkflowApplicationVariableSnapshotHookMetadata,
    )>,
    String,
> {
    let expected = WorkflowApplicationVariableSnapshotHookMetadata::from_run_step(input, resolved)?;
    let hook_id = expected.flow_hook_id();
    let Some(hook) = snapshot.hooks.get(&hook_id) else {
        return Ok(None);
    };
    let observed = serde_json::from_value::<WorkflowApplicationVariableSnapshotHookMetadata>(
        hook.metadata.clone(),
    )
    .map_err(|error| {
        format!("Workflow Application variable snapshot hook metadata is invalid: {error}")
    })?;
    observed.validate()?;
    if hook.hook_id != hook_id || hook.token != expected.flow_hook_token() || observed != expected {
        return Err("Workflow Application variable snapshot hook authority drifted".into());
    }
    Ok(Some((hook, observed)))
}

pub(super) fn application_variable_snapshot_payload(
    hook: &HookSnapshot,
    metadata: &WorkflowApplicationVariableSnapshotHookMetadata,
) -> Result<Option<WorkflowApplicationVariableSnapshotResumePayload>, String> {
    if hook.status != HookStatus::Received {
        return Ok(None);
    }
    let payload = hook.payload.as_ref().ok_or_else(|| {
        format!(
            "Workflow Application variable snapshot hook {:?} is received without a payload",
            hook.hook_id
        )
    })?;
    let payload =
        serde_json::from_value::<WorkflowApplicationVariableSnapshotResumePayload>(payload.clone())
            .map_err(|error| {
                format!("Workflow Application variable snapshot payload is invalid: {error}")
            })?;
    payload.validate(metadata)?;
    Ok(Some(payload))
}

pub(super) fn application_variable_write_hook<'a>(
    input: &WorkflowRunInput,
    resolved: &crate::modules::workflow::domain::ResolvedWorkflowRunStep,
    application_snapshot: &WorkflowApplicationVariableSnapshotResumePayload,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<
    Option<(
        &'a HookSnapshot,
        WorkflowApplicationVariableWriteHookMetadata,
    )>,
    String,
> {
    let hook_id = format!(
        "workflow-application-variable-write:{}:{}",
        resolved.plan.id,
        crate::modules::workflow::domain::WORKFLOW_APPLICATION_VARIABLE_STEP_ATTEMPT
    );
    let Some(hook) = snapshot.hooks.get(&hook_id) else {
        return Ok(None);
    };
    let observed = serde_json::from_value::<WorkflowApplicationVariableWriteHookMetadata>(
        hook.metadata.clone(),
    )
    .map_err(|error| {
        format!("Workflow Application variable write hook metadata is invalid: {error}")
    })?;
    observed.validate_run_step(input, resolved, application_snapshot)?;
    if hook.hook_id != hook_id || hook.token != observed.flow_hook_token() {
        return Err("Workflow Application variable write hook authority drifted".into());
    }
    Ok(Some((hook, observed)))
}

pub(super) fn application_answer_hook<'a>(
    input: &WorkflowRunInput,
    resolved: &crate::modules::workflow::domain::ResolvedWorkflowRunStep,
    snapshot: &'a WorkflowRunSnapshot,
) -> Result<Option<(&'a HookSnapshot, WorkflowApplicationAnswerHookMetadata)>, String> {
    let hook_id = format!(
        "workflow-application-answer:{}:{}",
        resolved.plan.id,
        crate::modules::workflow::domain::WORKFLOW_APPLICATION_ANSWER_STEP_ATTEMPT
    );
    let Some(hook) = snapshot.hooks.get(&hook_id) else {
        return Ok(None);
    };
    let observed =
        serde_json::from_value::<WorkflowApplicationAnswerHookMetadata>(hook.metadata.clone())
            .map_err(|error| {
                format!("Workflow Application Answer hook metadata is invalid: {error}")
            })?;
    observed.validate()?;
    let expected = WorkflowApplicationAnswerHookMetadata::from_run_step(
        input,
        resolved,
        observed.content.clone(),
    )?;
    if hook.hook_id != hook_id || hook.token != expected.flow_hook_token() || observed != expected {
        return Err("Workflow Application Answer hook authority drifted".into());
    }
    Ok(Some((hook, observed)))
}

pub(super) fn human_decision_hook<'a>(
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
