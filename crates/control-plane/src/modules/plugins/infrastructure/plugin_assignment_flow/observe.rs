use super::types::{ObserveInput, ObserveOutput, ObservedAssignment};
use super::PluginAssignmentFlowRuntime;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::plugins::domain::entities::PluginAssignment;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_use_core::{
    PluginDesiredState, PluginHostObservationRequest, PluginHostObservationStatus,
    PluginHostPackageState, PluginObservedState, PluginPackageId,
    PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA,
};
use chrono::{Duration, Utc};
use uuid::Uuid;

/// Deterministic namespace for observation command IDs derived from Operation + attempt.
const OBSERVE_COMMAND_NS: Uuid = Uuid::from_bytes([
    0xa3, 0x50, 0x1u8, 0x6f, 0x62, 0x73, 0x65, 0x72, 0x76, 0x65, 0x2d, 0x65, 0x6e, 0x71, 0x00,
    0x01,
]);

pub(super) fn observe_command_id(operation_id: Uuid, observation_attempt: u32) -> NodeCommandId {
    let mut bytes = operation_id.as_bytes().to_vec();
    bytes.extend_from_slice(&observation_attempt.to_le_bytes());
    NodeCommandId::from_uuid(Uuid::new_v5(&OBSERVE_COMMAND_NS, &bytes))
}

pub(super) async fn observe(
    runtime: &PluginAssignmentFlowRuntime,
    input: ObserveInput,
) -> a3s_flow::Result<ObserveOutput> {
    if input.observation_attempt == 0 {
        return Err(FlowError::Runtime(
            "plugin assignment observation attempt must be positive".into(),
        ));
    }
    let applied = *input.applied;
    let locked = applied
        .confirmed
        .stored
        .planned
        .authorized
        .resolved
        .locked
        .as_ref();
    let assignment = runtime
        .assignments
        .find(locked.organization_id, locked.assignment_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "plugin assignment observation reload failed: {error}"
            ))
        })?
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment disappeared before observation".into())
        })?;
    if assignment.organization_id != locked.organization_id
        || assignment.id != locked.assignment_id
        || assignment.current_operation_id != Some(locked.operation_id)
        || assignment.assignment_generation != locked.assignment_generation
        || assignment.registry_id != locked.registry_id
        || assignment.target_host_id != locked.target_host_id
    {
        return Ok(ObserveOutput::Terminal {
            reason: "plugin assignment drifted before observation".into(),
        });
    }

    let deadline_at = applied
        .confirmed
        .stored
        .planned
        .authorized
        .authorized_at
        .checked_add_signed(runtime.config.convergence_timeout)
        .ok_or_else(|| {
            FlowError::Runtime("plugin assignment observation deadline overflowed".into())
        })?;
    let now = Utc::now().max(applied.applied_at);
    if now >= deadline_at {
        return Ok(ObserveOutput::Terminal {
            reason: "plugin assignment observation timed out".into(),
        });
    }

    let command_id = observe_command_id(locked.operation_id.as_uuid(), input.observation_attempt);
    let command = match runtime
        .node_control
        .find_command(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host observation command: {error}"
            ))
        })? {
        Some(existing) => existing,
        None => {
            let request = PluginHostObservationRequest {
                schema: PLUGIN_HOST_OBSERVATION_REQUEST_SCHEMA.into(),
                request_id: format!(
                    "request:observe:{}:{}",
                    locked.operation_id.as_uuid(),
                    input.observation_attempt
                ),
                assignment_generation: locked.assignment_generation,
                capabilities_digest: applied
                    .confirmed
                    .stored
                    .planned
                    .authorized
                    .resolved
                    .capabilities_digest
                    .clone(),
                scope: assignment.workspace_scope.clone(),
                package_id: PluginPackageId::parse(assignment.package_id().as_str())
                    .map_err(|error| FlowError::Runtime(error.to_string()))?,
            };
            request
                .validate()
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "plugin assignment observation command TTL overflowed".into(),
                    )
                })?
                .min(deadline_at);
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: locked.target_host_id,
                    aggregate_id: locked.assignment_id.as_uuid(),
                    payload: NodeCommandPayload::PluginHostObserve {
                        request: Box::new(request),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: locked.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| {
                    FlowError::Runtime(format!(
                        "could not enqueue Plugin Host observation: {error}"
                    ))
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
            "Plugin Host observation command identity drifted on enqueue".into(),
        ));
    }
    let NodeCommandPayload::PluginHostObserve { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "Plugin Host observation command payload kind drifted".into(),
        ));
    };

    let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(locked.target_host_id, command_id)
        .await
        .map_err(|error| {
            FlowError::Runtime(format!(
                "could not load Plugin Host observation acknowledgement: {error}"
            ))
        })?
    else {
        if Utc::now() >= command.not_after {
            return Ok(ObserveOutput::Terminal {
                reason: "Plugin Host observation expired before acknowledgement".into(),
            });
        }
        return pending(
            "waiting for Plugin Host observation acknowledgement".into(),
            Utc::now().max(now),
            deadline_at.min(command.not_after),
            runtime.config.observation_poll,
        );
    };

    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => {
            let NodeCommandResult::PluginHostObserved {
                capabilities,
                observation,
            } = *result
            else {
                return Ok(ObserveOutput::Terminal {
                    reason: "Plugin Host observation returned an unexpected result".into(),
                });
            };
            observation
                .validate_for(request, &capabilities)
                .map_err(|error| FlowError::Runtime(error.to_string()))?;
            match observation.status {
                PluginHostObservationStatus::Unavailable { reason } => {
                    if Utc::now() >= deadline_at {
                        return Ok(ObserveOutput::Terminal {
                            reason: format!(
                                "Plugin Host observation remained unavailable ({reason:?})"
                            ),
                        });
                    }
                    pending(
                        format!("waiting for Plugin Host observation availability ({reason:?})"),
                        Utc::now().max(now),
                        deadline_at,
                        runtime.config.observation_poll,
                    )
                }
                PluginHostObservationStatus::Available { state } => {
                    match converge_observation(&assignment, &applied, &state) {
                        ConvergeDecision::Ready => {
                            Ok(ObserveOutput::Ready {
                                observed: Box::new(ObservedAssignment {
                                    applied: Box::new(applied),
                                    command_id,
                                    observed_desired: format!("{:?}", state.desired)
                                        .to_ascii_lowercase(),
                                    observed_state: format!("{:?}", state.observed)
                                        .to_ascii_lowercase(),
                                    capability_generation: state.capability_generation,
                                    package_generation: state.package_generation,
                                    observed_at: Utc::now(),
                                }),
                            })
                        }
                        ConvergeDecision::Pending(reason) => {
                            pending(
                                reason,
                                Utc::now().max(now),
                                deadline_at,
                                runtime.config.observation_poll,
                            )
                        }
                        ConvergeDecision::Terminal(reason) => Ok(ObserveOutput::Terminal { reason }),
                    }
                }
            }
        }
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(ObserveOutput::Terminal {
                reason: format!(
                    "Plugin Host observation {}: {}",
                    failure.code, failure.message
                ),
            })
        }
    }
}

enum ConvergeDecision {
    Ready,
    Pending(String),
    Terminal(String),
}

fn converge_observation(
    assignment: &PluginAssignment,
    applied: &super::types::AppliedAssignment,
    state: &PluginHostPackageState,
) -> ConvergeDecision {
    if state.desired != assignment.desired_state {
        return ConvergeDecision::Pending(format!(
            "waiting for Plugin Host desired state {:?} to match assignment {:?}",
            state.desired, assignment.desired_state
        ));
    }
    if state.capability_generation < applied.capability_generation {
        return ConvergeDecision::Pending(
            "waiting for Plugin Host capability generation to catch applied receipt".into(),
        );
    }
    match (assignment.desired_state, state.observed) {
        (PluginDesiredState::Enabled, PluginObservedState::Ready)
        | (PluginDesiredState::InstalledDisabled, PluginObservedState::Installed)
        | (PluginDesiredState::InstalledDisabled, PluginObservedState::Ready) => {
            if state.version.as_deref() != Some(assignment.selection.version.as_str())
                || state.package_digest.as_deref()
                    != Some(assignment.selection.package_digest.as_str())
                || state.manifest_digest.as_deref()
                    != Some(assignment.selection.manifest_digest.as_str())
                || state.selected_surfaces != assignment.selection.selected_surfaces
            {
                return ConvergeDecision::Pending(
                    "waiting for Plugin Host package and surface evidence to match the assignment"
                        .into(),
                );
            }
            ConvergeDecision::Ready
        }
        (PluginDesiredState::Absent, PluginObservedState::Removed) => ConvergeDecision::Ready,
        (
            _,
            PluginObservedState::Reconciling
            | PluginObservedState::Installed
            | PluginObservedState::Draining,
        ) => ConvergeDecision::Pending(format!(
            "waiting for Plugin Host observation to leave {:?}",
            state.observed
        )),
        (
            _,
            PluginObservedState::Broken
            | PluginObservedState::Incompatible
            | PluginObservedState::Degraded,
        ) => ConvergeDecision::Terminal(format!(
            "Plugin Host observation converged to {:?}",
            state.observed
        )),
        (desired, observed) => ConvergeDecision::Pending(format!(
            "waiting for Plugin Host observation {observed:?} to satisfy desired {desired:?}"
        )),
    }
}

fn pending(
    reason: String,
    now: chrono::DateTime<Utc>,
    deadline_at: chrono::DateTime<Utc>,
    poll: Duration,
) -> a3s_flow::Result<ObserveOutput> {
    let next_poll_at = now
        .checked_add_signed(poll)
        .ok_or_else(|| FlowError::Runtime("observation poll overflowed".into()))?
        .min(deadline_at);
    Ok(ObserveOutput::Pending {
        reason,
        next_poll_at,
        deadline_at,
    })
}
