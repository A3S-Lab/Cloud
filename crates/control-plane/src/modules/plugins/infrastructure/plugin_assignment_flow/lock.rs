use super::types::{LockOutput, LockedPluginAssignment, PluginAssignmentFlowInput};
use super::PluginAssignmentFlowRuntime;
use a3s_flow::FlowError;
use a3s_use_core::PluginDesiredState;
use chrono::Utc;

fn desired_state_label(desired_state: PluginDesiredState) -> &'static str {
    match desired_state {
        PluginDesiredState::Enabled => "enabled",
        PluginDesiredState::InstalledDisabled => "installed-disabled",
        PluginDesiredState::Absent => "absent",
    }
}

pub(super) async fn lock(
    runtime: &PluginAssignmentFlowRuntime,
    input: PluginAssignmentFlowInput,
) -> a3s_flow::Result<LockOutput> {
    let assignment = runtime
        .assignments
        .find(input.organization_id, input.assignment_id)
        .await
        .map_err(|error| FlowError::Runtime(format!("plugin assignment lock failed: {error}")))?
        .ok_or_else(|| FlowError::Runtime("plugin assignment was not found for lock".into()))?;

    if assignment.organization_id != input.organization_id
        || assignment.id != input.assignment_id
        || assignment.current_operation_id != Some(input.operation_id)
        || assignment.assignment_generation != input.assignment_generation
    {
        return Ok(LockOutput::Terminal {
            reason: "plugin assignment generation or operation identity drifted before lock".into(),
        });
    }

    let locked = LockedPluginAssignment {
        organization_id: assignment.organization_id,
        assignment_id: assignment.id,
        operation_id: input.operation_id,
        assignment_generation: assignment.assignment_generation,
        registry_id: assignment.registry_id,
        target_host_id: assignment.target_host_id,
        package_id: assignment.package_id().as_str().to_owned(),
        desired_state: desired_state_label(assignment.desired_state).into(),
        locked_at: Utc::now(),
    };

    Ok(LockOutput::Ready {
        locked: Box::new(locked),
    })
}
