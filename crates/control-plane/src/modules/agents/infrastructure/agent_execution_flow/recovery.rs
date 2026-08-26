use super::runtime::{observe_pending, provider_command_payload_matches};
use super::types::{DispatchedAgentExecution, ObserveOutput, PreparedAgentExecution};
use super::{flow_error, AgentExecutionFlowRuntime};
use crate::modules::agents::domain::{AgentCodeRunBinding, AgentExecution};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{AgentExecutionId, NodeCommandId};
use a3s_cloud_contracts::{AgentProviderCommandV1, NodeCommandPayload, RuntimeServiceEndpoint};
use a3s_flow::FlowError;
use a3s_runtime::contract::{RuntimeUnitClass, RuntimeUnitState, TransportProtocol};
use chrono::{DateTime, Utc};

pub(super) async fn begin(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    previous: DispatchedAgentExecution,
) -> a3s_flow::Result<ObserveOutput> {
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution lost its A3S Code binding".into()))?;
    if !binding.is_recovery_successor_of(&previous.prepared.binding, execution.id) {
        return Err(FlowError::Runtime(
            "Agent execution recovery changed its durable checkpoint identity".into(),
        ));
    }
    let Some(process) =
        active_runtime_process(runtime, binding, Utc::now().max(execution.updated_at)).await?
    else {
        return observe_pending(
            runtime,
            "waiting for the A3S Code Harness process before recovery",
            None,
        );
    };
    let checkpoint_run_id = previous.prepared.binding.identity().run_id.clone();
    let expected = command(runtime, &execution, &checkpoint_run_id)?;
    let command_id = command_id(execution.id, &checkpoint_run_id);
    let node_id = binding.node_id();
    let prepared = PreparedAgentExecution {
        organization_id: execution.organization_id,
        execution_id: execution.id,
        binding: binding.clone(),
        runtime_started_at_ms: Some(process.started_at_ms),
    };
    let node_command = match runtime
        .node_control
        .find_command(node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload A3S Code recovery command", error))?
    {
        Some(command) => command,
        None => {
            let issued_at = Utc::now().max(execution.updated_at);
            let not_after = issued_at
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("A3S Code recovery command deadline overflowed".into())
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
                                    flow_error("could not bind Agent provider recovery", error)
                                })?,
                        ),
                        command: Box::new(expected.clone()),
                    },
                    issued_at,
                    not_after,
                    correlation_id: execution.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue A3S Code recovery command", error))?
                .value
        }
    };
    validate_command(
        &execution,
        &prepared,
        &checkpoint_run_id,
        &expected,
        &node_command,
    )?;
    let dispatched = DispatchedAgentExecution {
        prepared: Box::new(prepared),
        command_id,
        acknowledgement_deadline: node_command.not_after,
        recovery_checkpoint_run_id: Some(checkpoint_run_id),
    };
    observe_pending(
        runtime,
        "waiting for the Code-owned recovery run",
        Some(dispatched),
    )
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ActiveRuntimeProcess {
    pub(super) started_at_ms: u64,
    pub(super) received_at: DateTime<Utc>,
}

pub(super) async fn active_runtime_process(
    runtime: &AgentExecutionFlowRuntime,
    binding: &AgentCodeRunBinding,
    now: DateTime<Utc>,
) -> a3s_flow::Result<Option<ActiveRuntimeProcess>> {
    let observation = runtime
        .node_control
        .latest_runtime_observation(
            binding.node_id(),
            binding.runtime_unit_id(),
            binding.runtime_generation(),
        )
        .await
        .map_err(|error| flow_error("could not load A3S Code Runtime observation", error))?;
    let Some(observation) = observation else {
        return Ok(None);
    };
    let runtime_observation = &observation.observation;
    if observation.node_id != binding.node_id()
        || runtime_observation.unit_id != binding.runtime_unit_id()
        || runtime_observation.generation != binding.runtime_generation()
        || runtime_observation.spec_digest != binding.runtime_spec_digest().as_str()
        || runtime_observation.class != RuntimeUnitClass::Service
    {
        return Err(FlowError::Runtime(
            "A3S Code Runtime observation changed its durable binding".into(),
        ));
    }
    if observation
        .received_at
        .checked_add_signed(runtime.config.heartbeat_timeout)
        .is_none_or(|fresh_until| fresh_until < now)
        || runtime_observation.state != RuntimeUnitState::Running
    {
        return Ok(None);
    }
    let endpoint =
        RuntimeServiceEndpoint::from_observation(runtime_observation, binding.service_port_name())
            .map_err(|error| flow_error("could not resolve A3S Code Runtime endpoint", error))?;
    if endpoint.protocol != TransportProtocol::Tcp {
        return Err(FlowError::Runtime(
            "A3S Code Harness Runtime endpoint is not TCP".into(),
        ));
    }
    let started_at_ms = runtime_observation.started_at_ms.ok_or_else(|| {
        FlowError::Runtime("A3S Code Harness Runtime has no process start time".into())
    })?;
    Ok(Some(ActiveRuntimeProcess {
        started_at_ms,
        received_at: observation.received_at,
    }))
}

pub(super) fn command(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    checkpoint_run_id: &str,
) -> a3s_flow::Result<AgentProviderCommandV1> {
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent execution has no provider binding".into()))?;
    let identity = binding
        .provider_identity()
        .map_err(|error| flow_error("could not bind Agent recovery provider identity", error))?;
    if identity.run_id != AgentCodeRunBinding::recovery_run_id(execution.id, checkpoint_run_id) {
        return Err(FlowError::Runtime(
            "Agent execution recovery run does not match its checkpoint".into(),
        ));
    }
    let profile = binding
        .provider()
        .map_err(|error| flow_error("could not restore Agent recovery provider", error))?;
    let provider = runtime
        .providers
        .provider_for_profile(profile)
        .map_err(|error| flow_error("could not resolve Agent recovery provider", error))?;
    provider
        .recover_command(
            format!(
                "agent-recover-{}",
                command_id(execution.id, checkpoint_run_id)
            ),
            identity,
            checkpoint_run_id.into(),
        )
        .map_err(|error| {
            flow_error(
                "Agent execution recovery is not a valid provider command",
                error,
            )
        })
}

pub(super) fn validate_command(
    execution: &AgentExecution,
    prepared: &PreparedAgentExecution,
    checkpoint_run_id: &str,
    expected: &AgentProviderCommandV1,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    if command.id != command_id(execution.id, checkpoint_run_id)
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
            "Agent provider recovery command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn command_id(execution_id: AgentExecutionId, checkpoint_run_id: &str) -> NodeCommandId {
    NodeCommandId::from_uuid(uuid::Uuid::new_v5(
        &execution_id.as_uuid(),
        format!("a3s-code-recover-v1:{checkpoint_run_id}").as_bytes(),
    ))
}
