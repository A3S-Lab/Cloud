use super::common::{completed, load_execution, next_poll, terminal, timestamp_millis};
use super::types::{
    CleanupDispatchInput, CleanupDispatchOutput, CleanupObserveInput, CleanupObserveOutput,
    CompletedExecution, ExecutionFlowInput,
};
use super::{flow_error, ExecutionFlowRuntime};
use crate::modules::executions::domain::{Execution, ExecutionStatus};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_runtime::contract::RuntimeActionRequest;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn dispatch(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: CleanupDispatchInput,
) -> a3s_flow::Result<CleanupDispatchOutput> {
    let flow = ExecutionFlowInput {
        organization_id: input.terminal.organization_id,
        execution_id: input.terminal.execution_id,
    };
    let mut execution = load_execution(runtime, run_id, &flow).await?;
    validate_terminal(&execution, &input.terminal)?;
    if execution.status.is_terminal() {
        return Ok(CleanupDispatchOutput::Completed {
            execution: completed(&execution)?,
        });
    }
    if execution.status != ExecutionStatus::CleanupPending {
        return Err(FlowError::Runtime(format!(
            "execution cannot clean up from {}",
            execution.status.as_str()
        )));
    }
    if execution.command_id.is_none() {
        return Ok(CleanupDispatchOutput::Completed {
            execution: complete(runtime, execution, Utc::now()).await?,
        });
    }
    let node_id = execution
        .node_id
        .ok_or_else(|| FlowError::Runtime("execution cleanup omitted its Runtime node".into()))?;
    let now = Utc::now().max(execution.updated_at);
    if now >= input.cleanup_deadline {
        return Err(FlowError::Runtime(
            "execution Runtime cleanup exceeded its independent deadline".into(),
        ));
    }
    let not_after = input
        .issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("execution cleanup command deadline overflowed".into()))?
        .min(input.cleanup_deadline);
    if now >= not_after {
        return Ok(CleanupDispatchOutput::Retry {
            reason: "execution cleanup command expired before dispatch".into(),
            next_attempt_at: now,
            deadline_at: input.cleanup_deadline,
        });
    }
    let command_id = cleanup_command_id(execution.id, input.attempt);
    if execution.cleanup_command_id == Some(command_id) {
        let command = runtime
            .node_control
            .find_command(node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload execution cleanup command", error))?
            .ok_or_else(|| FlowError::Runtime("execution cleanup command is missing".into()))?;
        validate_remove_command(&execution, input.attempt, &command)?;
        return Ok(CleanupDispatchOutput::Ready {
            dispatched: super::types::DispatchedCleanup {
                terminal: input.terminal,
                node_id,
                command_id,
                result_deadline: remove_result_deadline(&command)?.min(input.cleanup_deadline),
                cleanup_deadline: input.cleanup_deadline,
                attempt: input.attempt,
            },
        });
    }
    let payload = NodeCommandPayload::RuntimeRemove {
        request: RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("execution:{}:remove:{}", execution.id, input.attempt),
            unit_id: execution.runtime_unit_id(),
            generation: Execution::RUNTIME_GENERATION,
            deadline_at_ms: Some(timestamp_millis(not_after)?),
        },
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: execution.id.as_uuid(),
            payload,
            issued_at: input.issued_at,
            not_after,
            correlation_id: execution.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue execution cleanup command", error))?
        .value;
    validate_remove_command(&execution, input.attempt, &command)?;
    let expected = execution.aggregate_version;
    execution
        .record_cleanup_command(command.id, now)
        .map_err(|error| flow_error("could not record execution cleanup command", error))?;
    let execution = runtime
        .executions
        .save(execution, expected)
        .await
        .map_err(|error| flow_error("could not persist execution cleanup command", error))?;
    Ok(CleanupDispatchOutput::Ready {
        dispatched: super::types::DispatchedCleanup {
            terminal: input.terminal,
            node_id,
            command_id: execution.cleanup_command_id.ok_or_else(|| {
                FlowError::Runtime("execution cleanup omitted its Runtime command".into())
            })?,
            result_deadline: not_after,
            cleanup_deadline: input.cleanup_deadline,
            attempt: input.attempt,
        },
    })
}

pub(super) async fn observe(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: CleanupObserveInput,
) -> a3s_flow::Result<CleanupObserveOutput> {
    let flow = ExecutionFlowInput {
        organization_id: input.dispatched.terminal.organization_id,
        execution_id: input.dispatched.terminal.execution_id,
    };
    let execution = load_execution(runtime, run_id, &flow).await?;
    validate_terminal(&execution, &input.dispatched.terminal)?;
    if execution.status.is_terminal() {
        return Ok(CleanupObserveOutput::Completed {
            execution: completed(&execution)?,
        });
    }
    if execution.status != ExecutionStatus::CleanupPending
        || execution.node_id != Some(input.dispatched.node_id)
        || execution.cleanup_command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "execution cleanup observation changed its durable identity".into(),
        ));
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load execution cleanup result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::RuntimeRemoved { removal }
                    if removal.unit_id == execution.runtime_unit_id()
                        && removal.generation == Execution::RUNTIME_GENERATION =>
                {
                    return Ok(CleanupObserveOutput::Completed {
                        execution: complete(runtime, execution, acknowledgement.completed_at)
                            .await?,
                    })
                }
                _ => {
                    return retry(
                        runtime,
                        "execution cleanup completed without exact Runtime removal evidence",
                        input.dispatched.cleanup_deadline,
                    )
                }
            },
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return retry(
                    runtime,
                    &format!("{}: {}", failure.code, failure.message),
                    input.dispatched.cleanup_deadline,
                )
            }
        }
    }
    let now = Utc::now();
    if now >= input.dispatched.result_deadline {
        return retry(
            runtime,
            "execution cleanup command did not finish before its attempt deadline",
            input.dispatched.cleanup_deadline,
        );
    }
    Ok(CleanupObserveOutput::Pending {
        reason: "waiting for execution Runtime removal evidence".into(),
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.dispatched.result_deadline,
        )?,
        deadline_at: input.dispatched.result_deadline,
    })
}

async fn complete(
    runtime: &ExecutionFlowRuntime,
    mut execution: Execution,
    at: DateTime<Utc>,
) -> a3s_flow::Result<CompletedExecution> {
    if !execution.status.is_terminal() {
        let expected = execution.aggregate_version;
        execution
            .complete_cleanup(at.max(execution.updated_at))
            .map_err(|error| flow_error("could not complete execution cleanup", error))?;
        execution = runtime
            .executions
            .save(execution, expected)
            .await
            .map_err(|error| flow_error("could not persist execution completion", error))?;
    }
    completed(&execution)
}

fn retry(
    runtime: &ExecutionFlowRuntime,
    reason: &str,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<CleanupObserveOutput> {
    let now = Utc::now();
    if now >= deadline {
        return Err(FlowError::Runtime(
            "execution Runtime cleanup exceeded its independent deadline".into(),
        ));
    }
    Ok(CleanupObserveOutput::Retry {
        reason: super::common::bounded_reason(reason),
        next_attempt_at: next_poll(now, runtime.config.observation_poll, deadline)?,
        deadline_at: deadline,
    })
}

fn cleanup_command_id(
    execution_id: crate::modules::shared_kernel::domain::ExecutionId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &execution_id.as_uuid(),
        format!("runtime-remove:{attempt}").as_bytes(),
    ))
}

fn validate_terminal(
    execution: &Execution,
    expected: &super::types::TerminalExecution,
) -> a3s_flow::Result<()> {
    let actual = terminal(execution)?;
    if actual.organization_id != expected.organization_id
        || actual.execution_id != expected.execution_id
        || actual.outcome != expected.outcome
    {
        return Err(FlowError::Runtime(
            "execution cleanup changed its terminal intent".into(),
        ));
    }
    Ok(())
}

fn validate_remove_command(
    execution: &Execution,
    attempt: u32,
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "execution cleanup command is not a Runtime remove".into(),
        ));
    };
    if command.id != cleanup_command_id(execution.id, attempt)
        || command.node_id
            != execution
                .node_id
                .ok_or_else(|| FlowError::Runtime("execution omitted its cleanup node".into()))?
        || command.aggregate_id != execution.id.as_uuid()
        || command.correlation_id != execution.operation_id.as_uuid()
        || request.request_id != format!("execution:{}:remove:{attempt}", execution.id)
        || request.unit_id != execution.runtime_unit_id()
        || request.generation != Execution::RUNTIME_GENERATION
    {
        return Err(FlowError::Runtime(
            "execution Runtime remove command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn remove_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "execution cleanup command is not a Runtime remove".into(),
        ));
    };
    let millis = request.deadline_at_ms.ok_or_else(|| {
        FlowError::Runtime("execution Runtime remove omitted its deadline".into())
    })?;
    let millis = i64::try_from(millis)
        .map_err(|_| FlowError::Runtime("execution Runtime deadline is invalid".into()))?;
    DateTime::from_timestamp_millis(millis)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("execution Runtime deadline is invalid".into()))
}
