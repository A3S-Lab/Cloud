use super::flow_error;
use super::types::{DispatchedExecution, ExecutionFlowInput, ScheduledExecution};
use crate::modules::executions::domain::Execution;
use crate::modules::executions::infrastructure::project_execution_task;
use crate::modules::fleet::domain::entities::NodeCommand;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::NodeCommandPayload;
use a3s_flow::FlowError;
use a3s_runtime::contract::RuntimeUnitSpec;
use chrono::{DateTime, Utc};

pub(super) fn flow_input(scheduled: &ScheduledExecution) -> ExecutionFlowInput {
    ExecutionFlowInput {
        organization_id: scheduled.organization_id,
        execution_id: scheduled.execution_id,
    }
}

pub(super) fn validate_scheduled(
    execution: &Execution,
    scheduled: &ScheduledExecution,
) -> a3s_flow::Result<()> {
    let digest = scheduled
        .spec
        .digest()
        .map_err(|error| flow_error("scheduled execution Runtime Task is invalid", error))?;
    if execution.organization_id != scheduled.organization_id
        || execution.id != scheduled.execution_id
        || execution.node_id != Some(scheduled.node_id)
        || execution.runtime_spec_digest.as_deref() != Some(digest.as_str())
        || project_execution_task(execution)
            .map_err(|error| flow_error("could not replay execution Runtime Task", error))?
            != *scheduled.spec
    {
        return Err(FlowError::Runtime(
            "scheduled execution changed its Runtime identity".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_dispatched(
    execution: &Execution,
    dispatched: &DispatchedExecution,
) -> a3s_flow::Result<()> {
    validate_scheduled(execution, &dispatched.scheduled)?;
    if execution.command_id != Some(dispatched.command_id) {
        return Err(FlowError::Runtime(
            "dispatched execution changed its Runtime command identity".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_apply_command(
    execution: &Execution,
    spec: &RuntimeUnitSpec,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
        return Err(FlowError::Runtime(
            "execution command is not a Runtime apply".into(),
        ));
    };
    if command.id != NodeCommandId::from_uuid(execution.id.as_uuid())
        || command.node_id
            != execution
                .node_id
                .ok_or_else(|| FlowError::Runtime("execution omitted its Runtime node".into()))?
        || command.aggregate_id != execution.id.as_uuid()
        || command.correlation_id != execution.operation_id.as_uuid()
        || request.request_id != format!("execution:{}:apply", execution.id)
        || request.spec != *spec
    {
        return Err(FlowError::Runtime(
            "execution Runtime apply command changed its durable identity".into(),
        ));
    }
    Ok(())
}

pub(super) fn apply_result_deadline(command: &NodeCommand) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeApply { request, .. } = &command.payload else {
        return Err(FlowError::Runtime(
            "execution command is not a Runtime apply".into(),
        ));
    };
    let millis = request
        .deadline_at_ms
        .ok_or_else(|| FlowError::Runtime("execution Runtime apply omitted its deadline".into()))?;
    let millis = i64::try_from(millis)
        .map_err(|_| FlowError::Runtime("execution Runtime deadline is invalid".into()))?;
    DateTime::from_timestamp_millis(millis)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("execution Runtime deadline is invalid".into()))
}
