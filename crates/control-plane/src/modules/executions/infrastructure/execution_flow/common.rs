use super::types::{CompletedExecution, ExecutionFlowInput, TerminalExecution};
use super::{flow_error, ExecutionFlowRuntime};
use crate::modules::executions::domain::{Execution, ExecutionOutcome, ExecutionStatus};
use crate::modules::shared_kernel::domain::OperationId;
use a3s_flow::FlowError;
use chrono::{DateTime, Utc};

pub(super) async fn load_execution(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: &ExecutionFlowInput,
) -> a3s_flow::Result<Execution> {
    let operation_id =
        OperationId::from_uuid(uuid::Uuid::parse_str(run_id).map_err(|error| {
            FlowError::Runtime(format!("invalid execution Flow run ID: {error}"))
        })?);
    let execution = runtime
        .executions
        .find(input.organization_id, input.execution_id)
        .await
        .map_err(|error| flow_error("could not load execution", error))?
        .ok_or_else(|| FlowError::Runtime("execution no longer exists".into()))?;
    if operation_id != execution.operation_id
        || input.organization_id != execution.organization_id
        || input.execution_id != execution.id
    {
        return Err(FlowError::Runtime(
            "execution Flow input changed its durable identity".into(),
        ));
    }
    Ok(execution)
}

pub(super) async fn begin_cleanup(
    runtime: &ExecutionFlowRuntime,
    mut execution: Execution,
    outcome: ExecutionOutcome,
    at: DateTime<Utc>,
) -> a3s_flow::Result<TerminalExecution> {
    if execution.status.is_terminal() || execution.status == ExecutionStatus::CleanupPending {
        let persisted = execution
            .outcome
            .clone()
            .ok_or_else(|| FlowError::Runtime("execution terminal intent is missing".into()))?;
        if persisted != outcome {
            return Err(FlowError::Runtime(
                "execution terminal outcome changed during replay".into(),
            ));
        }
        return terminal(&execution);
    }
    let expected = execution.aggregate_version;
    execution
        .begin_cleanup(outcome, at.max(execution.updated_at))
        .map_err(|error| flow_error("could not begin execution cleanup", error))?;
    let execution = runtime
        .executions
        .save(execution, expected)
        .await
        .map_err(|error| flow_error("could not persist execution cleanup intent", error))?;
    terminal(&execution)
}

pub(super) fn terminal(execution: &Execution) -> a3s_flow::Result<TerminalExecution> {
    let outcome = execution
        .outcome
        .clone()
        .ok_or_else(|| FlowError::Runtime("execution terminal outcome is missing".into()))?;
    Ok(TerminalExecution {
        organization_id: execution.organization_id,
        execution_id: execution.id,
        outcome,
        terminal_at: execution.finished_at.unwrap_or(execution.updated_at),
        completed: execution.status.is_terminal(),
    })
}

pub(super) fn completed(execution: &Execution) -> a3s_flow::Result<CompletedExecution> {
    if !execution.status.is_terminal() {
        return Err(FlowError::Runtime(
            "execution cleanup completion is not terminal".into(),
        ));
    }
    Ok(CompletedExecution {
        execution_id: execution.id,
        status: execution.status,
        outcome: execution
            .outcome
            .clone()
            .ok_or_else(|| FlowError::Runtime("completed execution has no outcome".into()))?,
        finished_at: execution
            .finished_at
            .ok_or_else(|| FlowError::Runtime("completed execution has no timestamp".into()))?,
    })
}

pub(super) fn next_poll(
    now: DateTime<Utc>,
    interval: chrono::Duration,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<DateTime<Utc>> {
    now.checked_add_signed(interval)
        .map(|next| next.min(deadline))
        .ok_or_else(|| FlowError::Runtime("execution poll time overflowed".into()))
}

pub(super) fn timestamp_millis(value: DateTime<Utc>) -> a3s_flow::Result<u64> {
    u64::try_from(value.timestamp_millis())
        .map_err(|_| FlowError::Runtime("execution Runtime deadline is invalid".into()))
}

pub(super) fn bounded_reason(reason: impl AsRef<str>) -> String {
    let normalized = reason
        .as_ref()
        .chars()
        .map(|character| {
            if matches!(character, '\0' | '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        return "execution failed without a provider reason".into();
    }
    normalized.chars().take(16 * 1024).collect()
}
