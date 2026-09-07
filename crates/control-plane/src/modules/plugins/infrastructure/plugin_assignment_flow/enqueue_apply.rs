use super::types::{
    AppliedAssignment, EnqueueApplyInput, EnqueueApplyOutput,
};
use super::PluginAssignmentFlowRuntime;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_use_core::{PluginHostApplyRequest, PluginPackageId, PLUGIN_HOST_APPLY_REQUEST_SCHEMA};
use chrono::{Duration, Utc};
use uuid::Uuid;

/// Deterministic namespace for apply command IDs derived from the assignment Operation.
const ENQUEUE_APPLY_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x61, 0x70, 0x70, 0x6c, 0x79, 0x2d, 0x65, 0x6e, 0x71, 0x00, 0x00, 0x00,
    0x01,
]);

pub(super) fn enqueue_apply_command_id(operation_id: Uuid) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &ENQUEUE_APPLY_COMMAND_NS,
        operation_id.as_bytes(),
    ))
}

pub(super) async fn enqueue_apply(
    runtime: &PluginAssignmentFlowRuntime,
    input: EnqueueApplyInput,
) -> a3s_flow::Result<EnqueueApplyOutput> {
    let confirmed = *input.confirmed;
    let locked = confirmed.stored.planned.authorized.resolved.locked.as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment apply enqueue reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before apply enqueue".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(EnqueueApplyOutput::Terminal {
            reason: "plugin assignment drifted before apply enqueue".into(),
        });
    }

    let projection = runtime
        .projections
        .find(locked.organization_id, confirmed.stored.projection_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load plugin plan projection for apply: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin plan projection disappeared before apply enqueue".into())
        })?;
    if projection.plan_digest.as_str() != confirmed.stored.plan_digest
        || projection
            .confirmation_digest
            .as_ref()
            .map(|digest| digest.as_str().to_owned())
            != confirmed.confirmation_digest
        || projection.confirmation != confirmed.confirmation
    {
        return Ok(EnqueueApplyOutput::Terminal {
            reason: "plugin plan confirmation drifted before apply enqueue".into(),
        });
    }

    let deadline_at = confirmed
        .stored
        .planned
        .authorized
        .authorized_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment apply enqueue deadline overflowed".into())
        })?
        .min(projection.expires_at);
    let now = Utc::now()
        .max(confirmed.confirmed_at)
        .max(confirmed.stored.stored_at);
    if now >= deadline_at {
        return Ok(EnqueueApplyOutput::Terminal {
            reason: "plugin assignment apply enqueue timed out".into(),
        });
    }

    let command_id = enqueue_apply_command_id(locked.operation_id.as_uuid());
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host apply command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let request = PluginHostApplyRequest {
                schema: PLUGIN_HOST_APPLY_REQUEST_SCHEMA.into(),
                request_id: format!("request:apply:{}", locked.operation_id.as_uuid()),
                assignment_generation: locked.assignment_generation,
                capabilities_digest: confirmed
                    .stored
                    .planned
                    .authorized
                    .resolved
                    .capabilities_digest
                    .clone(),
                scope: assignment.workspace_scope.clone(),
                package_id: PluginPackageId::parse(assignment.package_id().as_str())
                    .map_err(|error| FlowError::Runtime(error.to_string()))?,
                operation_id: confirmed.stored.planned.use_operation_id.clone(),
                plan_digest: confirmed.stored.plan_digest.clone(),
                confirmation: confirmed.confirmation.clone(),
            };
            request
                .validate()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime("plugin assignment apply command TTL overflowed".into())
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostApply {
                        request: Box::new(request),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!("could not enqueue Plugin Host apply: {error}"))
                })?
                .value
        }
    };
    if command.id != command_id
        || command.node_id != locked.target_host_id
        || command.correlation_id != locked.operation_id.as_uuid()
        || command.generation() != locked.assignment_generation
    {
        return Err(FlowError::Runtime(
            "Plugin Host apply command identity drifted on enqueue".into(),
        ));
    }
    let NodeCommandPayload::PluginHostApply { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "Plugin Host apply command payload kind drifted".into(),
        ));
    };

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host apply acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(EnqueueApplyOutput::Terminal {
                reason: "Plugin Host apply expired before acknowledgement".into(),
            });
        }
        return pending(
            "waiting for Plugin Host apply acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        );
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostApplied {
                capabilities,
                applied,
            } = *result
            else {
                return Ok(EnqueueApplyOutput::Terminal {
                    reason: "Plugin Host apply returned an unexpected result".into(),
                });
            };
            applied
                .validate_for(request, &capabilities)
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            Ok(EnqueueApplyOutput::Ready {
                applied: Box::new(AppliedAssignment {
                    confirmed: Box::new(confirmed),
                    command_id,
                    request_id: applied.request_id,
                    operation_result_digest: applied.operation_result_digest,
                    capability_generation: applied.state.capability_generation,
                    package_generation: applied.state.package_generation,
                    applied_at: Utc::now(),
                }),
            })
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(EnqueueApplyOutput::Terminal {
                reason: format!("Plugin Host apply {}: {}", failure.code, failure.message),
            })
        }
    }
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<EnqueueApplyOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("apply enqueue poll overflowed".into()))?
        .min(deadline_at);
    Ok(EnqueueApplyOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}
