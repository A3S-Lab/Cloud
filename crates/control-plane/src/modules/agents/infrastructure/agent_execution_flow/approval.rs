use super::runtime::{
    accept_provider_result, fail_observation, observe_pending, provider_command_payload_matches,
};
use super::types::{
    DispatchedAgentApproval, DispatchedAgentExecution, ObserveOutput, PreparedAgentExecution,
};
use super::{flow_error, AgentExecutionFlowRuntime};
use crate::modules::agents::domain::{
    AgentApprovalCheckpoint, AgentApprovalCheckpointStatus, AgentCodeRunBinding, AgentExecution,
    AgentExecutionStatus, CancelActiveAgentApprovalCheckpointWrite,
    ExpireAgentApprovalCheckpointWrite, ResumeAgentApprovalCheckpointWrite,
};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentApprovalCheckpointId, NodeCommandId, Sha256Digest,
};
use a3s_cloud_contracts::{AgentProviderCommandV1, NodeCommandOutcome, NodeCommandPayload};
use a3s_flow::FlowError;
use chrono::{DateTime, Utc};

pub(super) async fn observe(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    mut dispatched: DispatchedAgentExecution,
) -> a3s_flow::Result<ObserveOutput> {
    if let Some(approval) = dispatched.approval.clone() {
        return observe_dispatched_approval(runtime, execution, dispatched, approval).await;
    }
    if execution.status != AgentExecutionStatus::AwaitingApproval {
        return Err(FlowError::Runtime(
            "Agent execution carried approval state while it was not awaiting approval".into(),
        ));
    }
    let Some(mut checkpoint) = runtime
        .agents
        .find_active_checkpoint(execution.organization_id, execution.id)
        .await
        .map_err(|error| flow_error("could not load active Agent approval checkpoint", error))?
    else {
        return fail_observation(
            runtime,
            execution,
            "awaiting Agent execution has no active approval checkpoint",
            Utc::now(),
        )
        .await;
    };
    validate_approval_checkpoint(&execution, &dispatched.prepared.binding, &checkpoint)?;
    let now = canonical_timestamp(Utc::now()).max(execution.updated_at);
    if checkpoint.status == AgentApprovalCheckpointStatus::Pending {
        if now < checkpoint.expires_at {
            return observe_pending_until(
                runtime,
                "waiting for an Agent Tool approval decision",
                checkpoint.expires_at,
                None,
            );
        }
        checkpoint = runtime
            .agents
            .expire_checkpoint(ExpireAgentApprovalCheckpointWrite {
                organization_id: checkpoint.organization_id,
                checkpoint_id: checkpoint.id,
                expected_version: checkpoint.aggregate_version,
                decision_id: checkpoint.deterministic_expiry_decision_id(),
                expired_at: now.max(checkpoint.expires_at),
            })
            .await
            .map_err(|error| flow_error("could not expire Agent approval checkpoint", error))?
            .checkpoint;
    }
    if !matches!(
        checkpoint.status,
        AgentApprovalCheckpointStatus::Approved
            | AgentApprovalCheckpointStatus::Denied
            | AgentApprovalCheckpointStatus::Expired
    ) {
        return Err(FlowError::Runtime(
            "active Agent approval checkpoint cannot produce a resume command".into(),
        ));
    }
    let expected = approval_command(runtime, &execution, &checkpoint)?;
    let command_id = approval_command_id(execution.id, checkpoint.id);
    let node_id = dispatched.prepared.binding.node_id();
    let command = match runtime
        .node_control
        .find_command(node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload Agent approval resume command", error))?
    {
        Some(command) => command,
        None => {
            let issued_at = now.max(checkpoint.updated_at);
            let not_after = issued_at
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("Agent approval resume command deadline overflowed".into())
                })?;
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id,
                    aggregate_id: execution.id.as_uuid(),
                    payload: NodeCommandPayload::AgentProviderCommand {
                        binding: Box::new(
                            dispatched
                                .prepared
                                .binding
                                .node_provider_runtime_binding(execution.id.as_uuid())
                                .map_err(|error| {
                                    flow_error(
                                        "could not bind Agent approval resume command",
                                        error,
                                    )
                                })?,
                        ),
                        command: Box::new(expected.clone()),
                    },
                    issued_at,
                    not_after,
                    correlation_id: execution.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    flow_error("could not enqueue Agent approval resume command", error)
                })?
                .value
        }
    };
    validate_approval_command(
        &execution,
        &dispatched.prepared,
        &checkpoint,
        &expected,
        &command,
    )?;
    dispatched.approval = Some(DispatchedAgentApproval {
        checkpoint_id: checkpoint.id,
        command_id,
        acknowledgement_deadline: command.not_after,
    });
    observe_pending_until(
        runtime,
        "waiting for the exact Agent approval resume command",
        command.not_after,
        Some(dispatched),
    )
}

async fn observe_dispatched_approval(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    mut dispatched: DispatchedAgentExecution,
    approval: DispatchedAgentApproval,
) -> a3s_flow::Result<ObserveOutput> {
    let Some(checkpoint) = runtime
        .agents
        .find_checkpoint(execution.organization_id, approval.checkpoint_id)
        .await
        .map_err(|error| flow_error("could not reload Agent approval checkpoint", error))?
    else {
        return fail_observation(
            runtime,
            execution,
            "dispatched Agent approval checkpoint no longer exists",
            Utc::now(),
        )
        .await;
    };
    validate_approval_checkpoint(&execution, &dispatched.prepared.binding, &checkpoint)?;
    if checkpoint.status == AgentApprovalCheckpointStatus::Cancelled {
        return fail_observation(
            runtime,
            execution,
            "dispatched Agent approval checkpoint was cancelled",
            Utc::now(),
        )
        .await;
    }
    let expected = approval_command(runtime, &execution, &checkpoint)?;
    let node_id = dispatched.prepared.binding.node_id();
    let command = runtime
        .node_control
        .find_command(node_id, approval.command_id)
        .await
        .map_err(|error| flow_error("could not load Agent approval resume command", error))?
        .ok_or_else(|| {
            FlowError::Runtime("Agent approval resume command no longer exists".into())
        })?;
    validate_approval_command(
        &execution,
        &dispatched.prepared,
        &checkpoint,
        &expected,
        &command,
    )?;
    if command.not_after != approval.acknowledgement_deadline {
        return Err(FlowError::Runtime(
            "Agent approval resume acknowledgement deadline changed".into(),
        ));
    }
    let now = canonical_timestamp(Utc::now()).max(execution.updated_at);
    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(node_id, approval.command_id)
        .await
        .map_err(|error| flow_error("could not load Agent approval resume result", error))?
    else {
        if now >= approval.acknowledgement_deadline {
            return fail_approval_observation(
                runtime,
                execution,
                "Agent approval resume command was not acknowledged before its deadline",
                approval.acknowledgement_deadline,
            )
            .await;
        }
        return observe_pending_until(
            runtime,
            "waiting for the exact Agent approval resume command",
            approval.acknowledgement_deadline,
            None,
        );
    };
    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            if let Err(error) =
                accept_provider_result(&dispatched.prepared.binding, &expected, &result)
            {
                return fail_approval_observation(
                    runtime,
                    execution,
                    &error,
                    acknowledgement.completed_at,
                )
                .await;
            }
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            return fail_approval_observation(
                runtime,
                execution,
                &format!("{}: {}", failure.code, failure.message),
                acknowledgement.completed_at,
            )
            .await;
        }
    }
    runtime
        .agents
        .mark_checkpoint_resumed(ResumeAgentApprovalCheckpointWrite {
            organization_id: checkpoint.organization_id,
            checkpoint_id: checkpoint.id,
            expected_version: checkpoint.aggregate_version,
            command_id: approval.command_id,
            command: expected,
            resumed_at: acknowledgement.completed_at.max(checkpoint.updated_at),
        })
        .await
        .map_err(|error| flow_error("could not settle Agent approval resume", error))?;
    dispatched.approval = None;
    observe_pending(
        runtime,
        "Agent provider accepted the exact Tool approval resume",
        Some(dispatched),
    )
}

fn approval_command(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
    checkpoint: &AgentApprovalCheckpoint,
) -> a3s_flow::Result<AgentProviderCommandV1> {
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| FlowError::Runtime("Agent approval has no provider binding".into()))?;
    validate_approval_checkpoint(execution, binding, checkpoint)?;
    let identity = binding
        .provider_identity()
        .map_err(|error| flow_error("could not bind Agent approval provider identity", error))?;
    let decision = checkpoint
        .protocol_decision_for(&identity)
        .map_err(|error| flow_error("could not restore exact Agent approval decision", error))?;
    let profile = binding
        .provider()
        .map_err(|error| flow_error("could not restore Agent approval provider", error))?;
    let provider = runtime
        .providers
        .provider_for_profile(profile)
        .map_err(|error| flow_error("could not resolve Agent approval provider", error))?;
    let command_id = approval_command_id(execution.id, checkpoint.id);
    provider
        .resume_command(
            format!("agent-approval-resume-{command_id}"),
            identity,
            decision,
        )
        .map_err(|error| flow_error("Agent approval is not a valid provider resume", error))
}

fn validate_approval_checkpoint(
    execution: &AgentExecution,
    binding: &AgentCodeRunBinding,
    checkpoint: &AgentApprovalCheckpoint,
) -> a3s_flow::Result<()> {
    checkpoint
        .validate()
        .map_err(|error| flow_error("invalid Agent approval checkpoint", error))?;
    let identity = binding
        .provider_identity()
        .map_err(|error| flow_error("could not validate Agent approval identity", error))?;
    let identity_digest = Sha256Digest::parse(
        identity
            .digest()
            .map_err(|error| flow_error("could not digest Agent approval identity", error))?,
    )
    .map_err(|error| flow_error("invalid Agent approval identity digest", error))?;
    let invocation_digest = Sha256Digest::parse(
        binding
            .require_invocation_profile()
            .and_then(|profile| profile.digest())
            .map_err(|error| flow_error("could not digest Agent approval invocation", error))?,
    )
    .map_err(|error| flow_error("invalid Agent approval invocation digest", error))?;
    if checkpoint.organization_id != execution.organization_id
        || checkpoint.conversation_id != execution.conversation_id
        || checkpoint.execution_id != execution.id
        || checkpoint.provider_run_identity_digest != identity_digest
        || checkpoint.invocation_profile_digest != invocation_digest
    {
        return Err(FlowError::Runtime(
            "Agent approval checkpoint changed its durable execution binding".into(),
        ));
    }
    Ok(())
}

fn validate_approval_command(
    execution: &AgentExecution,
    prepared: &PreparedAgentExecution,
    checkpoint: &AgentApprovalCheckpoint,
    expected: &AgentProviderCommandV1,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    if command.id != approval_command_id(execution.id, checkpoint.id)
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
            "Agent approval resume command changed its durable identity".into(),
        ));
    }
    Ok(())
}

async fn fail_approval_observation(
    runtime: &AgentExecutionFlowRuntime,
    execution: AgentExecution,
    reason: &str,
    failed_at: DateTime<Utc>,
) -> a3s_flow::Result<ObserveOutput> {
    runtime
        .agents
        .cancel_active_checkpoint(CancelActiveAgentApprovalCheckpointWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            cancelled_at: canonical_timestamp(failed_at).max(execution.updated_at),
        })
        .await
        .map_err(|error| flow_error("could not close failed Agent approval", error))?;
    fail_observation(runtime, execution, reason, failed_at).await
}

pub(super) async fn close_for_provider_restart(
    runtime: &AgentExecutionFlowRuntime,
    execution: &AgentExecution,
) -> a3s_flow::Result<Option<DateTime<Utc>>> {
    let failed_at = canonical_timestamp(Utc::now()).max(execution.updated_at);
    runtime
        .agents
        .cancel_active_checkpoint(CancelActiveAgentApprovalCheckpointWrite {
            organization_id: execution.organization_id,
            execution_id: execution.id,
            cancelled_at: failed_at,
        })
        .await
        .map(|write| write.map(|_| failed_at))
        .map_err(|error| {
            flow_error(
                "could not close Agent approval after provider restart",
                error,
            )
        })
}

fn observe_pending_until(
    runtime: &AgentExecutionFlowRuntime,
    reason: &str,
    deadline: DateTime<Utc>,
    dispatched: Option<DispatchedAgentExecution>,
) -> a3s_flow::Result<ObserveOutput> {
    let next_poll_at = Utc::now()
        .checked_add_signed(runtime.config.observation_poll)
        .ok_or_else(|| FlowError::Runtime("Agent observation poll time overflowed".into()))?
        .min(deadline);
    Ok(ObserveOutput::Pending {
        reason: reason.into(),
        next_poll_at,
        dispatched: dispatched.map(Box::new),
    })
}

fn approval_command_id(
    execution_id: crate::modules::shared_kernel::domain::AgentExecutionId,
    checkpoint_id: AgentApprovalCheckpointId,
) -> NodeCommandId {
    NodeCommandId::from_uuid(uuid::Uuid::new_v5(
        &execution_id.as_uuid(),
        format!("a3s-agent-approval-resume-v1:{checkpoint_id}").as_bytes(),
    ))
}
