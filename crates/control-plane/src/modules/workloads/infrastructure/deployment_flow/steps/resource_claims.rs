use super::super::types::{
    PrepareClaimStepInput, PrepareClaimStepOutput, ReleaseClaimStepInput, ReleaseClaimStepOutput,
};
use super::super::{flow_error, resource_claim_id, DeploymentFlowRuntime};
use super::{bounded_reason, next_poll, validate_resolved_deployment};
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{NodeCommandId, RepositoryError};
use crate::modules::workloads::domain::entities::{
    DeploymentStatus, ResourceClaim, ResourceClaimReleaseEvidence, ResourceClaimState,
};
use a3s_cloud_contracts::{
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NodeResourceClaimBinding,
    NodeResourceClaimPrepare, NodeResourceClaimRelease,
};
use a3s_flow::FlowError;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn prepare(
    runtime: &DeploymentFlowRuntime,
    input: PrepareClaimStepInput,
) -> a3s_flow::Result<PrepareClaimStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for resource preparation", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.node_id != Some(input.node_id) {
        return Err(FlowError::Runtime(
            "resource preparation does not match the scheduled deployment node".into(),
        ));
    }
    let cancellation_requested = matches!(
        deployment.status,
        DeploymentStatus::Cancelling
            | DeploymentStatus::CleanupPending
            | DeploymentStatus::Cancelled
    );
    let mut claim = runtime
        .resource_claims
        .find(
            input.resolved.organization_id,
            resource_claim_id(input.resolved.deployment_id),
        )
        .await
        .map_err(|error| flow_error("could not load deployment resource claim", error))?;
    validate_claim_identity(&claim, &input)?;

    if matches!(
        claim.state,
        ResourceClaimState::PreparedOnAgent | ResourceClaimState::BoundToRuntimeUnit
    ) {
        let binding = load_prepared_binding(runtime, &claim).await?;
        let binding_digest = binding
            .digest()
            .map_err(|error| flow_error("could not digest prepared resource binding", error))?;
        if claim.prepared_binding_digest.as_ref() != Some(&binding_digest) {
            return Err(FlowError::Runtime(
                "persisted resource preparation digest changed".into(),
            ));
        }
        if cancellation_requested {
            return Ok(PrepareClaimStepOutput::CancellationRequested);
        }
        return Ok(PrepareClaimStepOutput::Ready {
            node_id: claim.node_id,
            binding_digest,
            prepared_at: claim.prepared_at.ok_or_else(|| {
                FlowError::Runtime("prepared resource claim omitted its completion time".into())
            })?,
        });
    }
    if matches!(
        claim.state,
        ResourceClaimState::Releasing | ResourceClaimState::Released | ResourceClaimState::Orphaned
    ) {
        return if cancellation_requested {
            Ok(PrepareClaimStepOutput::CancellationRequested)
        } else {
            Ok(PrepareClaimStepOutput::Failed {
                reason: claim
                    .failure
                    .unwrap_or_else(|| "resource claim left the preparation lifecycle".into()),
            })
        };
    }
    if cancellation_requested && claim.state == ResourceClaimState::ReservedInDb {
        return Ok(PrepareClaimStepOutput::CancellationRequested);
    }

    let command = match claim.state {
        ResourceClaimState::ReservedInDb => {
            let issued_at = Utc::now().max(claim.updated_at);
            let not_after = issued_at
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("resource preparation command deadline overflowed".into())
                })?
                .min(input.resolved.convergence_deadline);
            if issued_at >= not_after {
                return Ok(PrepareClaimStepOutput::Failed {
                    reason: "resource preparation deadline expired before dispatch".into(),
                });
            }
            let inventory = runtime
                .node_control
                .current_resource_inventory(claim.node_id)
                .await
                .map_err(|error| {
                    flow_error(
                        "could not load current Agent inventory for resource preparation",
                        error,
                    )
                })?
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "scheduled node has no current Agent resource inventory".into(),
                    )
                })?;
            let binding = claim
                .node_binding(inventory.inventory.agent_instance_id)
                .map_err(|error| flow_error("could not build Agent resource binding", error))?;
            binding
                .validate_inventory(&inventory.inventory)
                .map_err(|error| {
                    flow_error(
                        "scheduled resource inventory is no longer current on the Agent",
                        error,
                    )
                })?;
            let command_id = prepare_command_id(&claim);
            let request = NodeResourceClaimPrepare {
                schema: NodeResourceClaimPrepare::SCHEMA.into(),
                claim_generation: claim.claim_generation,
                claim_digest: claim.claim_digest.clone(),
                binding,
            };
            let command = runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: claim.node_id,
                    aggregate_id: claim.id.as_uuid(),
                    payload: NodeCommandPayload::ResourceClaimPrepare {
                        request: Box::new(request),
                    },
                    issued_at,
                    not_after,
                    correlation_id: deployment.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue Agent resource preparation", error))?
                .value;
            validate_prepare_command(&command, &claim, deployment.operation_id.as_uuid())?;
            claim = runtime
                .resource_claims
                .begin_preparation(
                    claim.organization_id,
                    claim.id,
                    claim.aggregate_version,
                    command.id,
                    command.issued_at,
                )
                .await
                .map_err(|error| {
                    flow_error("could not persist Agent resource preparation", error)
                })?;
            command
        }
        ResourceClaimState::PreparingOnAgent => {
            let command_id = claim.prepare_command_id.ok_or_else(|| {
                FlowError::Runtime("preparing resource claim omitted its command".into())
            })?;
            let command = runtime
                .node_control
                .find_command(claim.node_id, command_id)
                .await
                .map_err(|error| flow_error("could not reload Agent resource preparation", error))?
                .ok_or_else(|| {
                    FlowError::Runtime("Agent resource preparation command is missing".into())
                })?;
            validate_prepare_command(&command, &claim, deployment.operation_id.as_uuid())?;
            command
        }
        _ => unreachable!("resource preparation states handled above"),
    };

    let acknowledgement = runtime
        .node_control
        .command_acknowledgement(claim.node_id, command.id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load Agent resource preparation acknowledgement",
                error,
            )
        })?;
    if let Some(acknowledgement) = acknowledgement {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                let NodeCommandResult::ResourceClaimPrepared { prepared } = result.as_ref() else {
                    return Err(FlowError::Runtime(
                        "resource preparation acknowledgement has the wrong result".into(),
                    ));
                };
                let NodeCommandPayload::ResourceClaimPrepare { request } = &command.payload else {
                    unreachable!("validated resource preparation command");
                };
                prepared.validate_for(request).map_err(|error| {
                    flow_error("Agent resource preparation evidence is invalid", error)
                })?;
                claim = runtime
                    .resource_claims
                    .record_prepared(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        command.id,
                        prepared.binding_digest.clone(),
                        acknowledgement.completed_at.max(claim.updated_at),
                    )
                    .await
                    .map_err(|error| {
                        flow_error(
                            "could not persist Agent resource preparation evidence",
                            error,
                        )
                    })?;
                if cancellation_requested {
                    return Ok(PrepareClaimStepOutput::CancellationRequested);
                }
                return Ok(PrepareClaimStepOutput::Ready {
                    node_id: claim.node_id,
                    binding_digest: claim.prepared_binding_digest.ok_or_else(|| {
                        FlowError::Runtime(
                            "prepared resource claim omitted its binding digest".into(),
                        )
                    })?,
                    prepared_at: claim.prepared_at.ok_or_else(|| {
                        FlowError::Runtime(
                            "prepared resource claim omitted its completion time".into(),
                        )
                    })?,
                });
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                let reason = bounded_reason(format!("{}: {}", failure.code, failure.message));
                orphan_claim(
                    runtime,
                    &claim,
                    format!("Agent resource preparation failed: {reason}"),
                    acknowledgement.completed_at,
                )
                .await?;
                return if cancellation_requested {
                    Ok(PrepareClaimStepOutput::CancellationRequested)
                } else {
                    Ok(PrepareClaimStepOutput::Failed { reason })
                };
            }
        }
    }

    let now = Utc::now();
    let deadline_at = command.not_after.min(input.resolved.convergence_deadline);
    if now >= deadline_at {
        let reason =
            "Agent resource preparation did not complete before its command deadline".to_string();
        orphan_claim(runtime, &claim, reason.clone(), now).await?;
        return if cancellation_requested {
            Ok(PrepareClaimStepOutput::CancellationRequested)
        } else {
            Ok(PrepareClaimStepOutput::Failed { reason })
        };
    }
    Ok(PrepareClaimStepOutput::Pending {
        reason: "waiting for durable Agent resource preparation evidence".into(),
        next_poll_at: next_poll(now, runtime.config.observation_poll, deadline_at)?,
        deadline_at,
    })
}

pub(super) async fn release(
    runtime: &DeploymentFlowRuntime,
    input: ReleaseClaimStepInput,
) -> a3s_flow::Result<ReleaseClaimStepOutput> {
    if input.released_after > input.deadline_at {
        return Err(FlowError::Runtime(
            "resource release prerequisite exceeds its deadline".into(),
        ));
    }
    let mut claim = match runtime
        .resource_claims
        .find(
            input.organization_id,
            resource_claim_id(input.deployment_id),
        )
        .await
    {
        Ok(claim) => claim,
        Err(RepositoryError::NotFound) => {
            return Ok(ReleaseClaimStepOutput::Ready {
                released_at: input.released_after,
            })
        }
        Err(error) => {
            return Err(flow_error(
                "could not load resource claim for release",
                error,
            ))
        }
    };
    if claim.organization_id != input.organization_id || claim.deployment_id != input.deployment_id
    {
        return Err(FlowError::Runtime(
            "resource release input does not own its claim".into(),
        ));
    }
    claim
        .validate()
        .map_err(|error| flow_error("resource claim for release is invalid", error))?;

    loop {
        match claim.state {
            ResourceClaimState::Released => {
                return Ok(ReleaseClaimStepOutput::Ready {
                    released_at: claim.released_at.ok_or_else(|| {
                        FlowError::Runtime(
                            "released resource claim omitted its evidence time".into(),
                        )
                    })?,
                })
            }
            ResourceClaimState::ReservedInDb => {
                claim = runtime
                    .resource_claims
                    .cancel_database_reservation(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        input.released_after.max(claim.updated_at),
                    )
                    .await
                    .map_err(|error| {
                        flow_error("could not cancel database resource reservation", error)
                    })?;
                continue;
            }
            ResourceClaimState::PreparingOnAgent => {
                claim = runtime
                    .resource_claims
                    .orphan(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        "resource release superseded an in-flight Agent preparation".into(),
                        Utc::now().max(claim.updated_at),
                    )
                    .await
                    .map_err(|error| {
                        flow_error("could not fence in-flight resource preparation", error)
                    })?;
            }
            ResourceClaimState::PreparedOnAgent
            | ResourceClaimState::BoundToRuntimeUnit
            | ResourceClaimState::Orphaned => {
                let now = Utc::now().max(claim.updated_at).max(input.released_after);
                if now >= input.deadline_at {
                    return Ok(ReleaseClaimStepOutput::Failed {
                        reason: "resource claim was not released before its deadline".into(),
                    });
                }
                let next_generation = claim.claim_generation.checked_add(1).ok_or_else(|| {
                    FlowError::Runtime("resource claim generation overflowed".into())
                })?;
                let command_id = release_command_id(&claim, next_generation);
                claim = runtime
                    .resource_claims
                    .begin_release(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        command_id,
                        now,
                    )
                    .await
                    .map_err(|error| {
                        flow_error("could not persist Agent resource release intent", error)
                    })?;
            }
            ResourceClaimState::Releasing => break,
        }
    }

    let binding = load_prepared_binding(runtime, &claim).await?;
    let command_id = claim.release_command_id.ok_or_else(|| {
        FlowError::Runtime("releasing resource claim omitted its command identity".into())
    })?;
    let issued_at = claim.release_requested_at.ok_or_else(|| {
        FlowError::Runtime("releasing resource claim omitted its request time".into())
    })?;
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("resource release command deadline overflowed".into()))?
        .min(input.deadline_at);
    let now = Utc::now();
    if now >= not_after {
        let reason = "Agent resource release command expired without durable evidence".to_string();
        claim = orphan_claim(runtime, &claim, reason.clone(), now).await?;
        return release_retry(runtime, &input, &claim, reason, now);
    }
    let request = NodeResourceClaimRelease {
        schema: NodeResourceClaimRelease::SCHEMA.into(),
        claim_generation: claim.claim_generation,
        claim_digest: claim.claim_digest.clone(),
        binding,
    };
    let command = match runtime
        .node_control
        .find_command(claim.node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload Agent resource release", error))?
    {
        Some(command) => command,
        None => {
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: claim.node_id,
                    aggregate_id: claim.id.as_uuid(),
                    payload: NodeCommandPayload::ResourceClaimRelease {
                        request: Box::new(request.clone()),
                    },
                    issued_at,
                    not_after,
                    correlation_id: input.deployment_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue Agent resource release", error))?
                .value
        }
    };
    validate_release_command(&command, &claim, &request, input.deployment_id.as_uuid())?;

    let acknowledgement = runtime
        .node_control
        .command_acknowledgement(claim.node_id, command.id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load Agent resource release acknowledgement",
                error,
            )
        })?;
    if let Some(acknowledgement) = acknowledgement {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                let NodeCommandResult::ResourceClaimReleased { released } = result.as_ref() else {
                    return Err(FlowError::Runtime(
                        "resource release acknowledgement has the wrong result".into(),
                    ));
                };
                released.validate_for(&request).map_err(|error| {
                    flow_error("Agent resource release evidence is invalid", error)
                })?;
                if released.released_at < input.released_after {
                    return Err(FlowError::Runtime(
                        "Agent resource release predates Runtime fencing evidence".into(),
                    ));
                }
                let evidence = ResourceClaimReleaseEvidence::AgentReleased {
                    command_id: command.id,
                    slots: released.slots.clone(),
                    evidence_digest: released.evidence_digest().map_err(|error| {
                        flow_error("could not digest Agent resource release evidence", error)
                    })?,
                    observed_at: released.released_at,
                };
                claim = runtime
                    .resource_claims
                    .record_released(
                        claim.organization_id,
                        claim.id,
                        claim.aggregate_version,
                        evidence,
                        acknowledgement.completed_at.max(claim.updated_at),
                    )
                    .await
                    .map_err(|error| {
                        flow_error("could not persist Agent resource release evidence", error)
                    })?;
                return Ok(ReleaseClaimStepOutput::Ready {
                    released_at: claim.released_at.ok_or_else(|| {
                        FlowError::Runtime(
                            "released resource claim omitted its evidence time".into(),
                        )
                    })?,
                });
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                let reason = bounded_reason(format!("{}: {}", failure.code, failure.message));
                claim = orphan_claim(
                    runtime,
                    &claim,
                    format!("Agent resource release failed: {reason}"),
                    acknowledgement.completed_at,
                )
                .await?;
                return release_retry(runtime, &input, &claim, reason, Utc::now());
            }
        }
    }

    let now = Utc::now();
    if now >= not_after {
        let reason =
            "Agent resource release did not complete before its command deadline".to_string();
        claim = orphan_claim(runtime, &claim, reason.clone(), now).await?;
        return release_retry(runtime, &input, &claim, reason, now);
    }
    Ok(ReleaseClaimStepOutput::Pending {
        reason: "waiting for durable Agent resource release evidence".into(),
        next_poll_at: next_poll(now, runtime.config.cleanup_poll, not_after)?,
        deadline_at: input.deadline_at,
    })
}

pub(super) async fn load_prepared_binding(
    runtime: &DeploymentFlowRuntime,
    claim: &ResourceClaim,
) -> a3s_flow::Result<NodeResourceClaimBinding> {
    let command_id = claim.prepare_command_id.ok_or_else(|| {
        FlowError::Runtime("issued resource claim omitted its preparation command".into())
    })?;
    let command = runtime
        .node_control
        .find_command(claim.node_id, command_id)
        .await
        .map_err(|error| flow_error("could not load durable resource preparation command", error))?
        .ok_or_else(|| {
            FlowError::Runtime("durable resource preparation command is missing".into())
        })?;
    if command.id != command_id
        || command.node_id != claim.node_id
        || command.aggregate_id != claim.id.as_uuid()
    {
        return Err(FlowError::Runtime(
            "resource preparation command identity changed".into(),
        ));
    }
    let NodeCommandPayload::ResourceClaimPrepare { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "resource preparation command has the wrong payload".into(),
        ));
    };
    let expected = claim
        .node_binding(request.binding.agent_instance_id)
        .map_err(|error| flow_error("could not validate durable resource binding", error))?;
    if request.binding != expected {
        return Err(FlowError::Runtime(
            "durable Agent resource binding changed".into(),
        ));
    }
    Ok(request.binding.clone())
}

fn validate_claim_identity(
    claim: &ResourceClaim,
    input: &PrepareClaimStepInput,
) -> a3s_flow::Result<()> {
    claim
        .validate()
        .map_err(|error| flow_error("deployment resource claim is invalid", error))?;
    if claim.organization_id != input.resolved.organization_id
        || claim.deployment_id != input.resolved.deployment_id
        || claim.workload_id != input.resolved.workload_id
        || claim.node_id != input.node_id
        || claim.runtime_unit_id != input.resolved.spec.unit_id
        || claim.runtime_generation != input.resolved.spec.generation
    {
        return Err(FlowError::Runtime(
            "deployment resource claim changed its exact placement".into(),
        ));
    }
    Ok(())
}

fn validate_prepare_command(
    command: &NodeCommand,
    claim: &ResourceClaim,
    correlation_id: Uuid,
) -> a3s_flow::Result<()> {
    if command.id != prepare_command_id(claim)
        || command.node_id != claim.node_id
        || command.aggregate_id != claim.id.as_uuid()
        || command.correlation_id != correlation_id
    {
        return Err(FlowError::Runtime(
            "Agent resource preparation command identity changed".into(),
        ));
    }
    let NodeCommandPayload::ResourceClaimPrepare { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "Agent resource preparation command has the wrong payload".into(),
        ));
    };
    let expected = claim
        .node_binding(request.binding.agent_instance_id)
        .map_err(|error| flow_error("could not validate Agent resource binding", error))?;
    if request.binding != expected
        || request.claim_generation != claim.claim_generation
        || request.claim_digest != claim.claim_digest
    {
        return Err(FlowError::Runtime(
            "Agent resource preparation command changed its exact claim".into(),
        ));
    }
    Ok(())
}

fn validate_release_command(
    command: &NodeCommand,
    claim: &ResourceClaim,
    request: &NodeResourceClaimRelease,
    correlation_id: Uuid,
) -> a3s_flow::Result<()> {
    if command.id
        != claim.release_command_id.ok_or_else(|| {
            FlowError::Runtime("releasing resource claim omitted its command identity".into())
        })?
        || command.node_id != claim.node_id
        || command.aggregate_id != claim.id.as_uuid()
        || command.correlation_id != correlation_id
    {
        return Err(FlowError::Runtime(
            "Agent resource release command identity changed".into(),
        ));
    }
    let NodeCommandPayload::ResourceClaimRelease { request: persisted } = &command.payload else {
        return Err(FlowError::Runtime(
            "Agent resource release command has the wrong payload".into(),
        ));
    };
    if persisted.as_ref() != request {
        return Err(FlowError::Runtime(
            "Agent resource release command changed its exact claim".into(),
        ));
    }
    Ok(())
}

async fn orphan_claim(
    runtime: &DeploymentFlowRuntime,
    claim: &ResourceClaim,
    reason: String,
    at: DateTime<Utc>,
) -> a3s_flow::Result<ResourceClaim> {
    runtime
        .resource_claims
        .orphan(
            claim.organization_id,
            claim.id,
            claim.aggregate_version,
            bounded_reason(reason),
            at.max(claim.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not orphan fenced resource claim", error))
}

fn release_retry(
    runtime: &DeploymentFlowRuntime,
    input: &ReleaseClaimStepInput,
    claim: &ResourceClaim,
    reason: String,
    now: DateTime<Utc>,
) -> a3s_flow::Result<ReleaseClaimStepOutput> {
    let now = now.max(claim.updated_at);
    if now >= input.deadline_at {
        return Ok(ReleaseClaimStepOutput::Failed { reason });
    }
    Ok(ReleaseClaimStepOutput::Pending {
        reason,
        next_poll_at: next_poll(now, runtime.config.cleanup_poll, input.deadline_at)?,
        deadline_at: input.deadline_at,
    })
}

fn prepare_command_id(claim: &ResourceClaim) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &claim.id.as_uuid(),
        format!("resource-claim-prepare:{}", claim.claim_generation).as_bytes(),
    ))
}

fn release_command_id(claim: &ResourceClaim, generation: u64) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &claim.id.as_uuid(),
        format!("resource-claim-release:{generation}").as_bytes(),
    ))
}
