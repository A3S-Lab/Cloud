use super::common::{
    begin_cleanup, bounded_reason, load_execution, next_poll, terminal, timestamp_millis,
};
use super::types::{
    DispatchInput, DispatchOutput, DispatchedExecution, ExecutionFlowInput, ObserveInput,
    ObserveOutput, ScheduleOutput, ScheduledExecution, TerminalExecution,
};
use super::validation::{
    apply_result_deadline, flow_input, validate_apply_command, validate_dispatched,
    validate_scheduled,
};
use super::{flow_error, ExecutionFlowRuntime};
use crate::modules::executions::domain::{Execution, ExecutionOutcome, ExecutionStatus};
use crate::modules::executions::infrastructure::project_execution_task;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_runtime::contract::{
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeObservation, RuntimeUnitSpec, RuntimeUnitState,
};
use chrono::{DateTime, Utc};

pub(super) async fn schedule(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: ExecutionFlowInput,
) -> a3s_flow::Result<ScheduleOutput> {
    let mut execution = load_execution(runtime, run_id, &input).await?;
    if execution.status.is_terminal() || execution.status == ExecutionStatus::CleanupPending {
        return Ok(ScheduleOutput::Terminal {
            terminal: terminal(&execution)?,
        });
    }
    if execution.status == ExecutionStatus::Cancelling {
        return Ok(ScheduleOutput::Terminal {
            terminal: begin_cleanup(runtime, execution, ExecutionOutcome::Cancelled, Utc::now())
                .await?,
        });
    }
    let spec = project_execution_task(&execution)
        .map_err(|error| flow_error("could not project execution Runtime Task", error))?;
    let spec_digest = spec
        .digest()
        .map_err(|error| flow_error("could not digest execution Runtime Task", error))?;
    let convergence_deadline = execution
        .requested_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| FlowError::Runtime("execution scheduling deadline overflowed".into()))?;
    if let Some(node_id) = execution.node_id {
        if execution.runtime_spec_digest.as_deref() != Some(spec_digest.as_str())
            || !matches!(
                execution.status,
                ExecutionStatus::Scheduled | ExecutionStatus::Running
            )
        {
            return Err(FlowError::Runtime(
                "scheduled execution Runtime identity changed during replay".into(),
            ));
        }
        return Ok(ScheduleOutput::Ready {
            scheduled: Box::new(ScheduledExecution {
                organization_id: execution.organization_id,
                execution_id: execution.id,
                node_id,
                spec: Box::new(spec),
                convergence_deadline,
            }),
        });
    }
    if execution.status != ExecutionStatus::Queued {
        return Err(FlowError::Runtime(format!(
            "execution cannot schedule from {}",
            execution.status.as_str()
        )));
    }
    let now = Utc::now().max(execution.updated_at);
    let mut nodes = runtime
        .nodes
        .list(execution.organization_id)
        .await
        .map_err(|error| flow_error("could not list execution Runtime nodes", error))?;
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        if !node.accepts_new_work_at(now, runtime.config.heartbeat_timeout) {
            continue;
        }
        let capabilities = match serde_json::from_value::<RuntimeCapabilities>(
            node.capabilities.document().clone(),
        ) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                tracing::warn!(
                    node_id = %node.id,
                    error = %error,
                    "ignoring invalid Runtime capabilities during execution scheduling"
                );
                continue;
            }
        };
        if !capabilities
            .missing_for(&spec)
            .map_err(|error| flow_error("could not match execution Runtime capabilities", error))?
            .is_empty()
        {
            continue;
        }
        let expected = execution.aggregate_version;
        execution
            .schedule(node.id, spec_digest.clone(), now)
            .map_err(|error| flow_error("could not schedule execution Runtime Task", error))?;
        let scheduled = runtime
            .executions
            .save(execution, expected)
            .await
            .map_err(|error| flow_error("could not persist execution Runtime schedule", error))?;
        return Ok(ScheduleOutput::Ready {
            scheduled: Box::new(ScheduledExecution {
                organization_id: scheduled.organization_id,
                execution_id: scheduled.id,
                node_id: scheduled.node_id.ok_or_else(|| {
                    FlowError::Runtime("scheduled execution omitted its Runtime node".into())
                })?,
                spec: Box::new(spec),
                convergence_deadline,
            }),
        });
    }
    if now >= convergence_deadline {
        return Ok(ScheduleOutput::Terminal {
            terminal: begin_cleanup(
                runtime,
                execution,
                ExecutionOutcome::Failed {
                    exit_code: None,
                    reason: "no ready node satisfied the isolated execution Runtime Task before its deadline".into(),
                },
                now,
            )
            .await?,
        });
    }
    Ok(ScheduleOutput::Pending {
        reason: "waiting for a node with Task, sandbox, Artifact, execution-timeout, and network-none capabilities".into(),
        next_poll_at: next_poll(now, runtime.config.observation_poll, convergence_deadline)?,
        deadline_at: convergence_deadline,
    })
}

pub(super) async fn dispatch(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: DispatchInput,
) -> a3s_flow::Result<DispatchOutput> {
    let flow = flow_input(&input.scheduled);
    let mut execution = load_execution(runtime, run_id, &flow).await?;
    validate_scheduled(&execution, &input.scheduled)?;
    if execution.status.is_terminal() || execution.status == ExecutionStatus::CleanupPending {
        return Ok(DispatchOutput::Terminal {
            terminal: terminal(&execution)?,
        });
    }
    if execution.status == ExecutionStatus::Cancelling {
        return Ok(DispatchOutput::Terminal {
            terminal: begin_cleanup(runtime, execution, ExecutionOutcome::Cancelled, Utc::now())
                .await?,
        });
    }
    if let Some(command_id) = execution.command_id {
        let command = runtime
            .node_control
            .find_command(input.scheduled.node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload execution Runtime command", error))?
            .ok_or_else(|| {
                FlowError::Runtime("dispatched execution Runtime command is missing".into())
            })?;
        validate_apply_command(&execution, &input.scheduled.spec, &command)?;
        return Ok(DispatchOutput::Ready {
            dispatched: Box::new(DispatchedExecution {
                scheduled: input.scheduled,
                command_id,
                result_deadline: apply_result_deadline(&command)?,
            }),
        });
    }
    if execution.status != ExecutionStatus::Scheduled {
        return Err(FlowError::Runtime(format!(
            "execution cannot dispatch from {}",
            execution.status.as_str()
        )));
    }
    let issued_at = Utc::now().max(execution.updated_at);
    let execution_timeout = chrono::Duration::milliseconds(
        i64::try_from(execution.template.resources.timeout_ms)
            .map_err(|_| FlowError::Runtime("execution timeout is invalid".into()))?,
    );
    let runtime_deadline = issued_at
        .checked_add_signed(execution_timeout)
        .ok_or_else(|| FlowError::Runtime("execution Runtime deadline overflowed".into()))?;
    let command_deadline = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("execution command deadline overflowed".into()))?;
    let result_deadline = runtime_deadline.min(command_deadline);
    if Utc::now() >= result_deadline {
        return Ok(DispatchOutput::Terminal {
            terminal: begin_cleanup(
                runtime,
                execution,
                ExecutionOutcome::Failed {
                    exit_code: None,
                    reason: "execution Runtime command expired before dispatch".into(),
                },
                Utc::now(),
            )
            .await?,
        });
    }
    let command_id = NodeCommandId::from_uuid(execution.id.as_uuid());
    let payload = NodeCommandPayload::RuntimeApply {
        request: Box::new(RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("execution:{}:apply", execution.id),
            deadline_at_ms: Some(timestamp_millis(result_deadline)?),
            spec: (*input.scheduled.spec).clone(),
        }),
        resource_claim: None,
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: input.scheduled.node_id,
            aggregate_id: execution.id.as_uuid(),
            payload,
            issued_at,
            not_after: result_deadline,
            correlation_id: execution.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue execution Runtime command", error))?
        .value;
    validate_apply_command(&execution, &input.scheduled.spec, &command)?;
    let expected = execution.aggregate_version;
    execution
        .dispatch(command.id, Utc::now().max(execution.updated_at))
        .map_err(|error| flow_error("could not mark execution Runtime dispatch", error))?;
    let dispatched = runtime
        .executions
        .save(execution, expected)
        .await
        .map_err(|error| flow_error("could not persist execution Runtime dispatch", error))?;
    Ok(DispatchOutput::Ready {
        dispatched: Box::new(DispatchedExecution {
            scheduled: input.scheduled,
            command_id: dispatched.command_id.ok_or_else(|| {
                FlowError::Runtime("dispatched execution omitted its Runtime command".into())
            })?,
            result_deadline,
        }),
    })
}

pub(super) async fn observe(
    runtime: &ExecutionFlowRuntime,
    run_id: &str,
    input: ObserveInput,
) -> a3s_flow::Result<ObserveOutput> {
    let flow = flow_input(&input.dispatched.scheduled);
    let execution = load_execution(runtime, run_id, &flow).await?;
    validate_dispatched(&execution, &input.dispatched)?;
    if execution.status.is_terminal() || execution.status == ExecutionStatus::CleanupPending {
        return Ok(ObserveOutput::Terminal {
            terminal: terminal(&execution)?,
        });
    }
    if execution.status == ExecutionStatus::Cancelling {
        return Ok(ObserveOutput::Terminal {
            terminal: begin_cleanup(runtime, execution, ExecutionOutcome::Cancelled, Utc::now())
                .await?,
        });
    }
    if let Some(record) = runtime
        .node_control
        .latest_runtime_observation(
            input.dispatched.scheduled.node_id,
            &input.dispatched.scheduled.spec.unit_id,
            input.dispatched.scheduled.spec.generation,
        )
        .await
        .map_err(|error| flow_error("could not load execution Runtime observation", error))?
    {
        if record.command_id != Some(input.dispatched.command_id) {
            return Err(FlowError::Runtime(
                "execution Runtime observation belongs to another command".into(),
            ));
        }
        if let Some(terminal) = consume_observation(
            runtime,
            execution.clone(),
            &input.dispatched.scheduled.spec,
            record.observation,
            record.received_at,
        )
        .await?
        {
            return Ok(ObserveOutput::Terminal { terminal });
        }
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(
            input.dispatched.scheduled.node_id,
            input.dispatched.command_id,
        )
        .await
        .map_err(|error| flow_error("could not load execution Runtime result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::RuntimeApplied { observation } => {
                    if let Some(terminal) = consume_observation(
                        runtime,
                        execution.clone(),
                        &input.dispatched.scheduled.spec,
                        *observation,
                        acknowledgement.completed_at,
                    )
                    .await?
                    {
                        return Ok(ObserveOutput::Terminal { terminal });
                    }
                }
                _ => {
                    return Err(FlowError::Runtime(
                        "execution apply command returned another result kind".into(),
                    ))
                }
            },
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return Ok(ObserveOutput::Terminal {
                    terminal: begin_cleanup(
                        runtime,
                        execution,
                        ExecutionOutcome::Failed {
                            exit_code: None,
                            reason: bounded_reason(format!(
                                "{}: {}",
                                failure.code, failure.message
                            )),
                        },
                        acknowledgement.completed_at,
                    )
                    .await?,
                })
            }
        }
    }
    let now = Utc::now();
    if now >= input.dispatched.result_deadline {
        return Ok(ObserveOutput::Terminal {
            terminal: begin_cleanup(
                runtime,
                execution,
                ExecutionOutcome::Failed {
                    exit_code: None,
                    reason: "execution Runtime Task did not finish before its deadline".into(),
                },
                now,
            )
            .await?,
        });
    }
    Ok(ObserveOutput::Pending {
        reason: "waiting for terminal execution Runtime observation".into(),
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.dispatched.result_deadline,
        )?,
        deadline_at: input.dispatched.result_deadline,
    })
}

async fn consume_observation(
    runtime: &ExecutionFlowRuntime,
    execution: Execution,
    spec: &RuntimeUnitSpec,
    observation: RuntimeObservation,
    completed_at: DateTime<Utc>,
) -> a3s_flow::Result<Option<TerminalExecution>> {
    observation
        .validate_against(spec)
        .map_err(|error| flow_error("execution Runtime observation is inconsistent", error))?;
    let outcome = match observation.state {
        RuntimeUnitState::Succeeded => ExecutionOutcome::Succeeded { exit_code: 0 },
        RuntimeUnitState::Failed => ExecutionOutcome::Failed {
            exit_code: None,
            reason: bounded_reason(
                observation
                    .failure
                    .map(|failure| format!("{}: {}", failure.code, failure.message))
                    .unwrap_or_else(|| "execution Runtime Task failed".into()),
            ),
        },
        state if state.is_terminal() => ExecutionOutcome::Failed {
            exit_code: None,
            reason: format!("execution Runtime Task terminated in unexpected state {state:?}"),
        },
        _ => return Ok(None),
    };
    begin_cleanup(runtime, execution, outcome, completed_at)
        .await
        .map(Some)
}
