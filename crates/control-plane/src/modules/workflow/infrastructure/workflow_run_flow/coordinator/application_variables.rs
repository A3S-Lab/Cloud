use super::*;

#[derive(Debug, Clone)]
pub(super) enum ObservedApplicationVariableHook {
    Snapshot {
        metadata: WorkflowApplicationVariableSnapshotHookMetadata,
        status: HookStatus,
    },
    Write {
        metadata: Box<WorkflowApplicationVariableWriteHookMetadata>,
        values: serde_json::Value,
        created_at: DateTime<Utc>,
        status: HookStatus,
    },
}

impl ObservedApplicationVariableHook {
    pub(super) const fn status(&self) -> HookStatus {
        match self {
            Self::Snapshot { status, .. } | Self::Write { status, .. } => *status,
        }
    }
}

pub(super) fn application_variable_hooks(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[a3s_flow::FlowEventEnvelope],
) -> Result<Vec<ObservedApplicationVariableHook>, WorkflowRunCoordinationError> {
    let Some(application) = record.run.execution_input.application_projection.as_ref() else {
        return Ok(Vec::new());
    };
    if application.variable_step_ids.is_empty() {
        return Ok(Vec::new());
    }
    let resolved_steps = record
        .run
        .execution_input
        .resolved_steps()
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
    let completed = super::super::projection::completed_workflow_steps(
        &record.run.execution_input,
        &resolved_steps,
        snapshot,
    )
    .map_err(WorkflowRunCoordinationError::Unavailable)?
    .completed;
    let mut hooks = Vec::new();
    for resolved in resolved_steps
        .iter()
        .filter(|resolved| application.is_variable_step(&resolved.plan.id))
    {
        let Some((snapshot_hook, snapshot_metadata)) =
            application_variable_snapshot_hook(&record.run.execution_input, resolved, snapshot)
                .map_err(WorkflowRunCoordinationError::Unavailable)?
        else {
            continue;
        };
        verify_application_hook_creation(
            &snapshot_metadata.flow_hook_id(),
            &snapshot_metadata.flow_hook_token(),
            &snapshot_metadata,
            history,
            "variable snapshot",
        )?;
        hooks.push(ObservedApplicationVariableHook::Snapshot {
            metadata: snapshot_metadata.clone(),
            status: snapshot_hook.status,
        });
        if !application.is_variable_assignment_step(&resolved.plan.id) {
            continue;
        }
        let Some(application_snapshot) =
            application_variable_snapshot_payload(snapshot_hook, &snapshot_metadata)
                .map_err(WorkflowRunCoordinationError::Unavailable)?
        else {
            continue;
        };
        let Some((write_hook, write_metadata)) = application_variable_write_hook(
            &record.run.execution_input,
            resolved,
            &application_snapshot,
            snapshot,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?
        else {
            continue;
        };
        let outputs = completed
            .iter()
            .filter(|(step_id, _)| *step_id != &resolved.plan.id)
            .map(|(step_id, result)| (step_id.clone(), result.output.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let composites = completed
            .iter()
            .filter(|(step_id, _)| *step_id != &resolved.plan.id)
            .filter_map(|(step_id, result)| {
                result
                    .composite_region_result
                    .clone()
                    .map(|region| (step_id.clone(), region))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let values = super::super::variables::application_assignment_values(
            &record.run.execution_input,
            &resolved.plan.id,
            &outputs,
            &composites,
            &application_snapshot,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        let expected_metadata = WorkflowApplicationVariableWriteHookMetadata::from_run_step(
            &record.run.execution_input,
            resolved,
            &application_snapshot,
            &values,
        )
        .map_err(WorkflowRunCoordinationError::Unavailable)?;
        if write_metadata != expected_metadata {
            return Err(WorkflowRunCoordinationError::Unavailable(
                "Workflow Application variable write hook values drifted".into(),
            ));
        }
        let write_created_at = verify_application_hook_creation(
            &write_metadata.flow_hook_id(),
            &write_metadata.flow_hook_token(),
            &write_metadata,
            history,
            "variable write",
        )?;
        hooks.push(ObservedApplicationVariableHook::Write {
            metadata: Box::new(write_metadata),
            values,
            created_at: write_created_at,
            status: write_hook.status,
        });
    }
    Ok(hooks)
}

fn verify_application_hook_creation<T: serde::Serialize>(
    hook_id: &str,
    token: &str,
    metadata: &T,
    history: &[a3s_flow::FlowEventEnvelope],
    label: &str,
) -> Result<DateTime<Utc>, WorkflowRunCoordinationError> {
    let expected_metadata = serde_json::to_value(metadata)
        .map_err(|error| WorkflowRunCoordinationError::Unavailable(error.to_string()))?;
    let matching = history
        .iter()
        .filter(|envelope| {
            matches!(
                &envelope.event,
                FlowEvent::HookCreated { hook_id: observed, .. } if observed == hook_id
            )
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(WorkflowRunCoordinationError::Unavailable(format!(
            "Workflow Application {label} hook {hook_id:?} must have exactly one creation event"
        )));
    }
    let FlowEvent::HookCreated {
        token: observed_token,
        metadata: observed_metadata,
        ..
    } = &matching[0].event
    else {
        return Err(WorkflowRunCoordinationError::Unavailable(format!(
            "Workflow Application {label} hook {hook_id:?} creation history is invalid"
        )));
    };
    if observed_token != token || observed_metadata != &expected_metadata {
        return Err(WorkflowRunCoordinationError::Unavailable(format!(
            "Workflow Application {label} hook {hook_id:?} creation authority drifted"
        )));
    }
    Ok(canonical_timestamp(matching[0].timestamp))
}
