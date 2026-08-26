use super::recovery;
use super::types::{
    AgentExecutionFlowInput, CompletedAgentExecution, DispatchInput, DispatchOutput,
    DispatchedAgentExecution, ObserveInput, ObserveOutput, PrepareOutput, PreparedAgentExecution,
};
use super::{flow_error, AgentExecutionFlowRuntime};
use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentEventContent, AgentExecution, AgentExecutionEventDraft,
    AgentExecutionEventKind, AppendAgentExecutionEventsWrite, BindAgentCodeRunWrite,
    RecoverAgentCodeRunWrite,
};
use crate::modules::agents::infrastructure::{accept_code_receipt, encode_code_command};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, NodeCommandId, OperationId, Sha256Digest,
};
use crate::modules::workloads::{project_runtime_spec, ActiveRuntimeTarget};
use a3s_cloud_contracts::{
    AgentProtocolRunIdentityV1, AgentProviderCommandV1, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, RuntimeServiceEndpoint, AGENT_PROTOCOL_V1,
};
use a3s_flow::FlowError;
use a3s_runtime::contract::TransportProtocol;
use chrono::{DateTime, Utc};

const ACTIVE_RUNTIME_TARGET_LIMIT: usize = 10_000;

pub(super) async fn prepare(
    runtime: &AgentExecutionFlowRuntime,
    run_id: &str,
    input: AgentExecutionFlowInput,
) -> a3s_flow::Result<PrepareOutput> {
    let execution = load_execution(runtime, run_id, &input).await?;
    if execution.status.is_terminal() {
        return Ok(PrepareOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    if execution.status == crate::modules::agents::domain::AgentExecutionStatus::Cancelling
        && execution.code.is_none()
    {
        let cancelled_at = Utc::now().max(execution.updated_at);
        let execution = cancel_execution(runtime, execution, cancelled_at).await?;
        return Ok(PrepareOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    if let Some(binding) = execution.code.clone() {
        return Ok(PrepareOutput::Ready {
            prepared: Box::new(PreparedAgentExecution {
                organization_id: execution.organization_id,
                execution_id: execution.id,
                binding,
                runtime_started_at_ms: None,
            }),
        });
    }

    let deadline_at = execution
        .requested_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("Agent Runtime convergence deadline overflowed".into())
        })?;
    let now = Utc::now().max(execution.updated_at);
    let conversation = runtime
        .agents
        .find_conversation(execution.organization_id, execution.conversation_id)
        .await
        .map_err(|error| flow_error("could not load Agent conversation", error))?
        .ok_or_else(|| {
            FlowError::Runtime("Agent execution conversation no longer exists".into())
        })?;
    let targets = runtime
        .workload_targets
        .list_active_runtime_targets(ACTIVE_RUNTIME_TARGET_LIMIT)
        .await
        .map_err(|error| flow_error("could not load active Agent Workloads", error))?;
    let mut candidates = targets.into_iter().filter(|target| {
        let Some(agent) = target.revision.agent_binding() else {
            return false;
        };
        target.workload.organization_id == execution.organization_id
            && target.workload.project_id == conversation.project_id
            && target.workload.environment_id == conversation.environment_id
            && agent.organization_id() == execution.organization_id
            && agent.asset_id() == execution.agent.asset_id()
            && agent.asset_release_id() == execution.agent.asset_release_id()
            && agent.build_run_id() == execution.agent.build_run_id()
    });
    let Some(target) = candidates.next() else {
        return pending_or_fail(
            runtime,
            execution,
            "waiting for one active Workload hosting the exact Agent release",
            now,
            deadline_at,
        )
        .await;
    };
    if candidates.next().is_some() {
        return pending_or_fail(
            runtime,
            execution,
            "multiple active Workloads host the Agent release; Cloud will not invent an Agent placement choice",
            now,
            deadline_at,
        )
        .await;
    }

    let (readiness, runtime_started_at_ms) =
        match ready_binding(runtime, &execution, &target, now).await {
            Ok(readiness) => readiness,
            Err(reason) => {
                return pending_or_fail(runtime, execution, &reason, now, deadline_at).await;
            }
        };
    let write = runtime
        .agents
        .bind_code_run(BindAgentCodeRunWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            binding: readiness,
        })
        .await
        .map_err(|error| flow_error("could not bind Agent execution to A3S Code", error))?;
    Ok(PrepareOutput::Ready {
        prepared: Box::new(PreparedAgentExecution {
            organization_id: write.execution.organization_id,
            execution_id: write.execution.id,
            binding: write.execution.code.ok_or_else(|| {
                FlowError::Runtime("bound Agent execution omitted its A3S Code identity".into())
            })?,
            runtime_started_at_ms: Some(runtime_started_at_ms),
        }),
    })
}

pub(super) async fn dispatch(
    runtime: &AgentExecutionFlowRuntime,
    run_id: &str,
    input: DispatchInput,
) -> a3s_flow::Result<DispatchOutput> {
    let flow = flow_input(&input.prepared);
    let execution = load_execution(runtime, run_id, &flow).await?;
    if execution.status.is_terminal() {
        return Ok(DispatchOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    validate_prepared(&execution, &input.prepared)?;
    let command_id = NodeCommandId::from_uuid(execution.id.as_uuid());
    let node_id = input.prepared.binding.node_id();
    let existing = runtime
        .node_control
        .find_command(node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload A3S Code command", error))?;
    if execution.status == crate::modules::agents::domain::AgentExecutionStatus::Cancelling
        && existing.is_none()
    {
        let cancelled_at = Utc::now().max(execution.updated_at);
        let execution = cancel_execution(runtime, execution, cancelled_at).await?;
        return Ok(DispatchOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    let expected = start_command(runtime, &execution).await?;
    let command = match existing {
        Some(command) => command,
        None => {
            let issued_at = Utc::now().max(execution.updated_at);
            let not_after = issued_at
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| FlowError::Runtime("A3S Code command deadline overflowed".into()))?;
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id,
                    aggregate_id: execution.id.as_uuid(),
                    payload: NodeCommandPayload::AgentProviderCommand {
                        binding: Box::new(
                            input
                                .prepared
                                .binding
                                .node_provider_runtime_binding(execution.id.as_uuid())
                                .map_err(|error| {
                                    flow_error("could not bind Agent provider command", error)
                                })?,
                        ),
                        command: Box::new(expected.clone()),
                    },
                    issued_at,
                    not_after,
                    correlation_id: execution.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue A3S Code command", error))?
                .value
        }
    };
    validate_start_command(&execution, &input.prepared, &expected, &command)?;
    Ok(DispatchOutput::Ready {
        dispatched: Box::new(DispatchedAgentExecution {
            prepared: input.prepared,
            command_id,
            acknowledgement_deadline: command.not_after,
            recovery_checkpoint_run_id: None,
        }),
    })
}

pub(super) async fn observe(
    runtime: &AgentExecutionFlowRuntime,
    run_id: &str,
    input: ObserveInput,
) -> a3s_flow::Result<ObserveOutput> {
    let flow = flow_input(&input.dispatched.prepared);
    let execution = load_execution(runtime, run_id, &flow).await?;
    if execution.status.is_terminal() {
        return Ok(ObserveOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    let current = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution lost its A3S Code binding".into()))?;
    if !current.has_same_run_binding(&input.dispatched.prepared.binding) {
        if current.is_recovery_successor_of(&input.dispatched.prepared.binding, execution.id) {
            return recovery::begin(runtime, execution, *input.dispatched).await;
        }
        return Err(FlowError::Runtime(
            "prepared Agent execution changed its durable identity".into(),
        ));
    }
    validate_prepared(&execution, &input.dispatched.prepared)?;
    let node_id = input.dispatched.prepared.binding.node_id();
    let command = runtime
        .node_control
        .find_command(node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load A3S Code command", error))?
        .ok_or_else(|| FlowError::Runtime("A3S Code command no longer exists".into()))?;
    let expected = dispatched_command(runtime, &execution, &input.dispatched).await?;
    validate_dispatched_command(&execution, &input.dispatched, &expected, &command)?;
    if command.not_after != input.dispatched.acknowledgement_deadline {
        return Err(FlowError::Runtime(
            "A3S Code command acknowledgement deadline changed".into(),
        ));
    }
    let cancelling =
        execution.status == crate::modules::agents::domain::AgentExecutionStatus::Cancelling;
    let acknowledged = if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load A3S Code command result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                if let Err(error) =
                    accept_provider_result(&input.dispatched.prepared.binding, &expected, &result)
                {
                    return fail_observation(
                        runtime,
                        execution,
                        &error,
                        acknowledgement.completed_at,
                    )
                    .await;
                }
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return fail_observation(
                    runtime,
                    execution,
                    &format!("{}: {}", failure.code, failure.message),
                    acknowledgement.completed_at,
                )
                .await
            }
        }
        true
    } else if Utc::now() >= input.dispatched.acknowledgement_deadline {
        return fail_observation(
            runtime,
            execution,
            "A3S Code command was not acknowledged before its deadline",
            input.dispatched.acknowledgement_deadline,
        )
        .await;
    } else {
        false
    };

    if acknowledged || cancelling {
        let process = recovery::active_runtime_process(
            runtime,
            &input.dispatched.prepared.binding,
            Utc::now().max(execution.updated_at),
        )
        .await?;
        if process.is_none() && acknowledged && !cancelling {
            return observe_pending(
                runtime,
                "waiting for the bound A3S Code Harness process to become ready",
                None,
            );
        }
        match (input.dispatched.prepared.runtime_started_at_ms, process) {
            (None, Some(process)) if !cancelling => {
                let mut dispatched = *input.dispatched;
                dispatched.prepared.runtime_started_at_ms = Some(process.started_at_ms);
                return observe_pending(
                    runtime,
                    "recorded the bound A3S Code Harness process incarnation",
                    Some(dispatched),
                );
            }
            (Some(started_at_ms), Some(process)) if started_at_ms != process.started_at_ms => {
                let write = runtime
                    .agents
                    .recover_code_run(RecoverAgentCodeRunWrite {
                        organization_id: execution.organization_id,
                        execution_id: execution.id,
                        expected_binding: input.dispatched.prepared.binding.clone(),
                        recovered_at: process.received_at.max(execution.updated_at),
                    })
                    .await
                    .map_err(|error| {
                        flow_error("could not recover the restarted A3S Code provider", error)
                    })?;
                return recovery::begin(runtime, write.execution, *input.dispatched).await;
            }
            _ => {}
        }
    }
    if cancelling {
        return observe_cancellation(runtime, execution, &input.dispatched.prepared).await;
    }
    observe_pending(
        runtime,
        "waiting for the Code-owned run to reach a terminal state",
        None,
    )
}

pub(super) fn observe_pending(
    runtime: &AgentExecutionFlowRuntime,
    reason: &str,
    dispatched: Option<DispatchedAgentExecution>,
) -> a3s_flow::Result<ObserveOutput> {
    Ok(ObserveOutput::Pending {
        reason: reason.into(),
        next_poll_at: Utc::now()
            .checked_add_signed(runtime.config.observation_poll)
            .ok_or_else(|| FlowError::Runtime("Agent observation poll time overflowed".into()))?,
        dispatched: dispatched.map(Box::new),
    })
}

async fn observe_cancellation(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    prepared: &PreparedAgentExecution,
) -> a3s_flow::Result<ObserveOutput> {
    let expected = cancel_command(runtime, &execution)?;
    let command_id = cancel_command_id(execution.id, &expected.identity().run_id);
    let node_id = prepared.binding.node_id();
    let command = match runtime
        .node_control
        .find_command(node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload A3S Code cancel command", error))?
    {
        Some(command) => command,
        None => {
            let issued_at = Utc::now().max(execution.updated_at);
            let not_after = issued_at
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("A3S Code cancel command deadline overflowed".into())
                })?;
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id,
                    aggregate_id: execution.id.as_uuid(),
                    payload: NodeCommandPayload::AgentProviderCommand {
                        binding: Box::new(
                            prepared
                                .binding
                                .node_provider_runtime_binding(execution.id.as_uuid())
                                .map_err(|error| {
                                    flow_error("could not bind Agent provider cancellation", error)
                                })?,
                        ),
                        command: Box::new(expected.clone()),
                    },
                    issued_at,
                    not_after,
                    correlation_id: execution.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue A3S Code cancel command", error))?
                .value
        }
    };
    validate_cancel_command(&execution, prepared, &expected, &command)?;
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(node_id, command_id)
        .await
        .map_err(|error| flow_error("could not load A3S Code cancel result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                if let Err(error) = accept_provider_result(&prepared.binding, &expected, &result) {
                    return fail_observation(
                        runtime,
                        execution,
                        &error,
                        acknowledgement.completed_at,
                    )
                    .await;
                }
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return fail_observation(
                    runtime,
                    execution,
                    &format!("{}: {}", failure.code, failure.message),
                    acknowledgement.completed_at,
                )
                .await;
            }
        }
    } else if Utc::now() >= command.not_after {
        return fail_observation(
            runtime,
            execution,
            "A3S Code cancel command was not acknowledged before its deadline",
            command.not_after,
        )
        .await;
    }
    Ok(ObserveOutput::Pending {
        reason: "waiting for the Code-owned cancellation event page".into(),
        next_poll_at: Utc::now()
            .checked_add_signed(runtime.config.observation_poll)
            .ok_or_else(|| FlowError::Runtime("Agent cancellation poll time overflowed".into()))?,
        dispatched: None,
    })
}

async fn ready_binding(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    target: &ActiveRuntimeTarget,
    now: DateTime<Utc>,
) -> Result<(AgentCodeRunBinding, u64), String> {
    let node_id = target
        .replica_binding
        .node_id
        .filter(|node_id| Some(*node_id) == target.deployment.node_id)
        .ok_or_else(|| "Agent Workload has no exact placed Runtime replica".to_owned())?;
    if target.replica_binding.workload_id != target.workload.id
        || target.replica_binding.revision_id != target.revision.id
        || target.replica_binding.deployment_id != target.deployment.id
    {
        return Err("Agent Workload Runtime binding changed its durable identity".into());
    }
    let template = target.revision.resolved_template()?;
    if template.artifact.digest != execution.agent.artifact_digest().as_str() {
        return Err("Agent Workload artifact does not match the execution release".into());
    }
    let service_port_name = template
        .health
        .as_ref()
        .map(|health| health.port_name.clone())
        .ok_or_else(|| "Agent Workload does not declare the Code Harness health port".to_owned())?;
    let spec = project_runtime_spec(&target.revision)?;
    let observation = runtime
        .node_control
        .latest_runtime_observation(node_id, &spec.unit_id, spec.generation)
        .await
        .map_err(|error| format!("could not load Agent Runtime observation: {error}"))?
        .ok_or_else(|| "Agent Runtime has no observation yet".to_owned())?;
    observation.observation.validate_against(&spec)?;
    if observation
        .received_at
        .checked_add_signed(runtime.config.heartbeat_timeout)
        .is_none_or(|fresh_until| fresh_until < now)
        || !observation.observation.converges(&spec)
    {
        return Err("Agent Runtime is not recently observed ready".into());
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(&observation.observation, &service_port_name)?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err("A3S Code Harness Runtime endpoint is not TCP".into());
    }
    let runtime_started_at_ms = observation
        .observation
        .started_at_ms
        .ok_or_else(|| "A3S Code Harness Runtime has no process start time".to_owned())?;
    let spec_digest = Sha256Digest::parse(spec.digest()?)?;
    let binding = AgentCodeRunBinding::new_with_provider(
        runtime.provider.profile().clone(),
        node_id,
        target.workload.id,
        target.revision.id,
        target.deployment.id,
        target.replica_binding.replica_id,
        spec.unit_id,
        spec.generation,
        spec_digest,
        service_port_name,
        AgentProtocolRunIdentityV1 {
            schema: AgentProtocolRunIdentityV1::SCHEMA.into(),
            protocol: AGENT_PROTOCOL_V1.into(),
            agent_release_identity: execution.agent.artifact_digest().as_str().into(),
            session_id: format!("agent-conversation-{}", execution.conversation_id),
            run_id: format!("agent-execution-{}", execution.id),
        },
        now,
    )?;
    Ok((binding, runtime_started_at_ms))
}

async fn start_command(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
) -> a3s_flow::Result<AgentProviderCommandV1> {
    let event = runtime
        .agents
        .find_execution_request(execution.organization_id, execution.id)
        .await
        .map_err(|error| flow_error("could not load Agent execution input", error))?
        .ok_or_else(|| FlowError::Runtime("Agent execution input no longer exists".into()))?;
    if event.execution_id != execution.id
        || event.conversation_id != execution.conversation_id
        || event.kind != AgentExecutionEventKind::ExecutionRequested
    {
        return Err(FlowError::Runtime(
            "Agent execution input changed its durable identity".into(),
        ));
    }
    let prompt = match event.content.value() {
        serde_json::Value::String(prompt) => prompt.clone(),
        input => serde_json::to_string(input)?,
    };
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution has no provider binding".into()))?;
    runtime
        .provider
        .start_command(
            format!("agent-execution-{}-start", execution.id),
            binding
                .provider_identity()
                .map_err(|error| flow_error("could not bind Agent provider identity", error))?,
            prompt,
        )
        .map_err(|error| flow_error("Agent execution input is not a valid provider start", error))
}

async fn dispatched_command(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    dispatched: &DispatchedAgentExecution,
) -> a3s_flow::Result<AgentProviderCommandV1> {
    match dispatched.recovery_checkpoint_run_id.as_deref() {
        Some(checkpoint_run_id) => recovery::command(runtime, execution, checkpoint_run_id),
        None => start_command(runtime, execution).await,
    }
}

fn cancel_command(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
) -> a3s_flow::Result<AgentProviderCommandV1> {
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution has no provider binding".into()))?;
    let identity = binding
        .provider_identity()
        .map_err(|error| flow_error("could not bind Agent provider identity", error))?;
    let command_id = cancel_command_id(execution.id, &identity.run_id);
    runtime
        .provider
        .cancel_command(
            format!("agent-cancel-{command_id}"),
            identity,
            "Cloud Agent execution cancellation requested".into(),
        )
        .map_err(|error| {
            flow_error(
                "Agent execution cancellation is not a valid provider command",
                error,
            )
        })
}

fn validate_start_command(
    execution: &AgentExecution,
    prepared: &PreparedAgentExecution,
    expected: &AgentProviderCommandV1,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    if command.id != NodeCommandId::from_uuid(execution.id.as_uuid())
        || command.node_id != prepared.binding.node_id()
        || command.aggregate_id != execution.id.as_uuid()
        || command.correlation_id != execution.operation_id.as_uuid()
        || !provider_command_payload_matches(
            &prepared.binding,
            execution.id.as_uuid(),
            expected,
            &command.payload,
        )?
    {
        return Err(FlowError::Runtime(
            "Agent provider start command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn validate_dispatched_command(
    execution: &AgentExecution,
    dispatched: &DispatchedAgentExecution,
    expected: &AgentProviderCommandV1,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    match dispatched.recovery_checkpoint_run_id.as_deref() {
        Some(checkpoint_run_id) => recovery::validate_command(
            execution,
            &dispatched.prepared,
            checkpoint_run_id,
            expected,
            command,
        ),
        None => validate_start_command(execution, &dispatched.prepared, expected, command),
    }
}

fn validate_cancel_command(
    execution: &AgentExecution,
    prepared: &PreparedAgentExecution,
    expected: &AgentProviderCommandV1,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    if command.id != cancel_command_id(execution.id, &expected.identity().run_id)
        || command.node_id != prepared.binding.node_id()
        || command.aggregate_id != execution.id.as_uuid()
        || command.correlation_id != execution.operation_id.as_uuid()
        || !provider_command_payload_matches(
            &prepared.binding,
            execution.id.as_uuid(),
            expected,
            &command.payload,
        )?
    {
        return Err(FlowError::Runtime(
            "Agent provider cancel command changed its durable identity".into(),
        ));
    }
    Ok(())
}

pub(super) fn provider_command_payload_matches(
    binding: &AgentCodeRunBinding,
    execution_id: uuid::Uuid,
    expected: &AgentProviderCommandV1,
    payload: &NodeCommandPayload,
) -> a3s_flow::Result<bool> {
    match payload {
        NodeCommandPayload::AgentProviderCommand {
            binding: actual_binding,
            command: actual,
        } => Ok(**actual_binding
            == binding
                .node_provider_runtime_binding(execution_id)
                .map_err(|error| flow_error("could not validate Agent provider binding", error))?
            && **actual == *expected),
        NodeCommandPayload::CodeAgentCommand {
            binding: actual_binding,
            command: actual,
        } => {
            let expected_native = encode_code_command(binding, expected).map_err(|error| {
                flow_error("could not validate legacy native Code command", error)
            })?;
            Ok(
                **actual_binding == binding.node_runtime_binding(execution_id)
                    && **actual == expected_native,
            )
        }
        _ => Ok(false),
    }
}

fn accept_provider_result(
    binding: &AgentCodeRunBinding,
    command: &AgentProviderCommandV1,
    result: &NodeCommandResult,
) -> Result<(), String> {
    match result {
        NodeCommandResult::AgentProviderCommandAccepted { receipt } => {
            receipt.validate_for(&binding.provider()?.profile()?, command)
        }
        NodeCommandResult::CodeAgentCommandAccepted { receipt } => {
            accept_code_receipt(binding, command, receipt).map(|_| ())
        }
        _ => Err("Agent provider command returned another result kind".into()),
    }
}

fn cancel_command_id(
    execution_id: crate::modules::shared_kernel::domain::AgentExecutionId,
    run_id: &str,
) -> NodeCommandId {
    NodeCommandId::from_uuid(uuid::Uuid::new_v5(
        &execution_id.as_uuid(),
        format!("a3s-code-cancel-v1:{run_id}").as_bytes(),
    ))
}

fn validate_prepared(
    execution: &AgentExecution,
    prepared: &PreparedAgentExecution,
) -> a3s_flow::Result<()> {
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution lost its A3S Code binding".into()))?;
    if execution.organization_id != prepared.organization_id
        || execution.id != prepared.execution_id
        || !binding.has_same_run_binding(&prepared.binding)
    {
        return Err(FlowError::Runtime(
            "prepared Agent execution changed its durable identity".into(),
        ));
    }
    Ok(())
}

async fn pending_or_fail(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    reason: &str,
    now: DateTime<Utc>,
    deadline_at: DateTime<Utc>,
) -> a3s_flow::Result<PrepareOutput> {
    if now >= deadline_at {
        let execution = fail_execution(runtime, execution, reason, deadline_at).await?;
        return Ok(PrepareOutput::Terminal {
            completed: completed(&execution)?,
        });
    }
    Ok(PrepareOutput::Pending {
        reason: reason.into(),
        next_poll_at: now
            .checked_add_signed(runtime.config.observation_poll)
            .map(|next| next.min(deadline_at))
            .ok_or_else(|| FlowError::Runtime("Agent preparation poll time overflowed".into()))?,
        deadline_at,
    })
}

async fn fail_observation(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    reason: &str,
    failed_at: DateTime<Utc>,
) -> a3s_flow::Result<ObserveOutput> {
    let execution = fail_execution(runtime, execution, reason, failed_at).await?;
    Ok(ObserveOutput::Terminal {
        completed: completed(&execution)?,
    })
}

async fn fail_execution(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    reason: &str,
    failed_at: DateTime<Utc>,
) -> a3s_flow::Result<AgentExecution> {
    if execution.status.is_terminal() {
        return Ok(execution);
    }
    let reason = bounded_reason(reason);
    let occurred_at = failed_at.max(execution.updated_at);
    let event = AgentExecutionEventDraft::new(
        AgentExecutionEventKind::ExecutionFailed,
        AgentEventContent::inline_json(serde_json::json!({"reason": reason}))
            .map_err(|error| flow_error("could not encode Agent failure", error))?,
        occurred_at,
    )
    .map_err(|error| flow_error("could not create Agent failure", error))?;
    let body = serde_json::to_vec(&serde_json::json!({
        "organizationId": execution.organization_id,
        "executionId": execution.id,
        "occurredAt": occurred_at,
        "event": {
            "kind": event.kind,
            "content": event.content.value(),
        },
    }))?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/agent-executions/{}/flow-terminal",
            execution.organization_id, execution.id
        ),
        execution.operation_id.to_string(),
        &body,
    )
    .map_err(|error| flow_error("could not bind Agent failure idempotency", error))?;
    runtime
        .agents
        .append_events(AppendAgentExecutionEventsWrite {
            organization_id: execution.organization_id,
            conversation_id: execution.conversation_id,
            execution_id: execution.id,
            events: vec![event],
            idempotency,
        })
        .await
        .map(|write| write.execution)
        .map_err(|error| flow_error("could not persist Agent execution failure", error))
}

async fn cancel_execution(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    cancelled_at: DateTime<Utc>,
) -> a3s_flow::Result<AgentExecution> {
    if execution.status.is_terminal() {
        return Ok(execution);
    }
    let occurred_at = cancelled_at.max(execution.updated_at);
    let event = AgentExecutionEventDraft::new(
        AgentExecutionEventKind::ExecutionCancelled,
        AgentEventContent::inline_json(serde_json::json!({}))
            .map_err(|error| flow_error("could not encode Agent cancellation", error))?,
        occurred_at,
    )
    .map_err(|error| flow_error("could not create Agent cancellation", error))?;
    let body = serde_json::to_vec(&serde_json::json!({
        "organizationId": execution.organization_id,
        "executionId": execution.id,
        "occurredAt": occurred_at,
        "event": {
            "kind": event.kind,
            "content": event.content.value(),
        },
    }))?;
    let idempotency = IdempotencyRequest::new(
        format!(
            "organizations/{}/agent-executions/{}/flow-terminal",
            execution.organization_id, execution.id
        ),
        execution.operation_id.to_string(),
        &body,
    )
    .map_err(|error| flow_error("could not bind Agent cancellation idempotency", error))?;
    runtime
        .agents
        .append_events(AppendAgentExecutionEventsWrite {
            organization_id: execution.organization_id,
            conversation_id: execution.conversation_id,
            execution_id: execution.id,
            events: vec![event],
            idempotency,
        })
        .await
        .map(|write| write.execution)
        .map_err(|error| flow_error("could not persist Agent execution cancellation", error))
}

async fn load_execution(
    runtime: &AgentExecutionFlowRuntime,
    run_id: &str,
    input: &AgentExecutionFlowInput,
) -> a3s_flow::Result<AgentExecution> {
    let operation_id = OperationId::from_uuid(
        uuid::Uuid::parse_str(run_id)
            .map_err(|error| FlowError::Runtime(format!("invalid Agent Flow run ID: {error}")))?,
    );
    let execution = runtime
        .agents
        .find_execution(input.organization_id, input.execution_id)
        .await
        .map_err(|error| flow_error("could not load Agent execution", error))?
        .ok_or_else(|| FlowError::Runtime("Agent execution no longer exists".into()))?;
    if operation_id != execution.operation_id
        || input.organization_id != execution.organization_id
        || input.execution_id != execution.id
    {
        return Err(FlowError::Runtime(
            "Agent Flow input changed its durable identity".into(),
        ));
    }
    Ok(execution)
}

fn flow_input(prepared: &PreparedAgentExecution) -> AgentExecutionFlowInput {
    AgentExecutionFlowInput {
        organization_id: prepared.organization_id,
        execution_id: prepared.execution_id,
    }
}

fn completed(execution: &AgentExecution) -> a3s_flow::Result<CompletedAgentExecution> {
    if !execution.status.is_terminal() {
        return Err(FlowError::Runtime("Agent execution is not terminal".into()));
    }
    Ok(CompletedAgentExecution {
        execution_id: execution.id,
        status: execution.status,
        finished_at: execution
            .finished_at
            .ok_or_else(|| FlowError::Runtime("terminal Agent execution has no time".into()))?,
    })
}

fn bounded_reason(reason: &str) -> String {
    const MAX_REASON_BYTES: usize = 16 * 1024;

    let normalized = reason
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
        return "Agent execution failed without a reason".into();
    }
    if normalized.len() <= MAX_REASON_BYTES {
        return normalized.into();
    }
    let mut end = MAX_REASON_BYTES;
    while !normalized.is_char_boundary(end) {
        end -= 1;
    }
    normalized[..end].into()
}
