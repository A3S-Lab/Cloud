use super::super::types::{
    CleanupDispatchStepInput, CleanupDispatchStepOutput, CleanupObserveStepInput,
    CleanupObserveStepOutput, CompleteCancellationStepInput, CompleteCancellationStepOutput,
    DispatchedCleanup,
};
use super::super::{cancel_database_reservation, flow_error, DeploymentFlowRuntime};
use super::{
    bounded_reason, next_poll, timestamp_millis, validate_resolved_deployment,
    validate_resolved_replica_binding,
};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use crate::modules::workloads::domain::entities::DeploymentStatus;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_flow::FlowError;
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeInspection, RuntimeUnitState};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CleanupAction {
    Stop,
    Remove,
}

impl CleanupAction {
    const fn name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Remove => "remove",
        }
    }

    fn command_id(
        self,
        deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
        attempt: u32,
    ) -> NodeCommandId {
        NodeCommandId::from_uuid(Uuid::new_v5(
            &deployment_id.as_uuid(),
            format!("runtime-{}:{attempt}", self.name()).as_bytes(),
        ))
    }

    fn payload(
        self,
        deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
        attempt: u32,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        deadline: DateTime<Utc>,
    ) -> a3s_flow::Result<NodeCommandPayload> {
        let request = RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("deployment:{deployment_id}:{}:{attempt}", self.name()),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            deadline_at_ms: Some(timestamp_millis(deadline)?),
        };
        Ok(match self {
            Self::Stop => NodeCommandPayload::RuntimeStop { request },
            Self::Remove => NodeCommandPayload::RuntimeRemove { request },
        })
    }

    fn result_matches(
        self,
        result: &a3s_cloud_contracts::NodeCommandResult,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
    ) -> bool {
        match (self, result) {
            (
                Self::Stop,
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::NotFound { .. },
                },
            ) => true,
            (
                Self::Stop,
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::Found { observation, .. },
                },
            ) => observation.state == RuntimeUnitState::Stopped,
            (Self::Remove, a3s_cloud_contracts::NodeCommandResult::RuntimeRemoved { removal }) => {
                removal.unit_id == spec.unit_id && removal.generation == spec.generation
            }
            _ => false,
        }
    }
}

pub(super) async fn dispatch_cleanup(
    runtime: &DeploymentFlowRuntime,
    input: CleanupDispatchStepInput,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
    dispatch(runtime, input, CleanupAction::Stop).await
}

pub(super) async fn dispatch_removal(
    runtime: &DeploymentFlowRuntime,
    input: CleanupDispatchStepInput,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
    dispatch(runtime, input, CleanupAction::Remove).await
}

async fn dispatch(
    runtime: &DeploymentFlowRuntime,
    input: CleanupDispatchStepInput,
    action: CleanupAction,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
    let mut deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for cleanup dispatch", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.status == DeploymentStatus::Cancelled {
        return Ok(CleanupDispatchStepOutput::NotRequired {
            cleaned_at: deployment.cancelled_at.unwrap_or(deployment.updated_at),
        });
    }
    if !matches!(
        deployment.status,
        DeploymentStatus::Cancelling | DeploymentStatus::CleanupPending
    ) {
        return Err(FlowError::Runtime(format!(
            "deployment cannot clean up from {}",
            deployment.status.as_str()
        )));
    }
    let cancellation_requested_at = deployment.cancellation_requested_at.ok_or_else(|| {
        FlowError::Runtime("cancelling deployment omitted its request time".into())
    })?;
    let cleanup_deadline = cancellation_requested_at
        .checked_add_signed(runtime.config.cleanup_timeout)
        .ok_or_else(|| FlowError::Runtime("deployment cleanup deadline overflowed".into()))?;
    let now = Utc::now().max(deployment.updated_at);
    if now >= cleanup_deadline {
        return Ok(CleanupDispatchStepOutput::Failed {
            reason: "Runtime cleanup did not complete before its independent deadline".into(),
        });
    }
    if deployment.command_id.is_none() {
        return Ok(CleanupDispatchStepOutput::NotRequired { cleaned_at: now });
    }
    let replica_binding = runtime
        .workloads
        .find_deployment_replica_binding(deployment.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not load replica binding for cleanup", error))?;
    validate_resolved_replica_binding(&replica_binding, &deployment, &input.resolved)?;
    let node_id = deployment.node_id.ok_or_else(|| {
        FlowError::Runtime("dispatched deployment cleanup omitted its node".into())
    })?;
    let issued_at = input.issued_at.unwrap_or(cancellation_requested_at);
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("cleanup command deadline overflowed".into()))?;
    let runtime_deadline = issued_at
        .checked_add_signed(runtime.config.runtime_stop_timeout)
        .ok_or_else(|| FlowError::Runtime("Runtime stop deadline overflowed".into()))?;
    let result_deadline = cleanup_deadline.min(not_after).min(runtime_deadline);
    if now >= result_deadline {
        return Ok(CleanupDispatchStepOutput::Retry {
            reason: format!(
                "cleanup attempt expired before Runtime {} dispatch",
                action.name()
            ),
            next_attempt_at: now,
            deadline_at: cleanup_deadline,
        });
    }

    let command_id = action.command_id(deployment.id, input.attempt);
    if deployment.cleanup_command_id == Some(command_id) {
        let command = runtime
            .node_control
            .find_command(node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload Runtime cleanup command", error))?
            .ok_or_else(|| FlowError::Runtime("Runtime cleanup command is missing".into()))?;
        if command.aggregate_id != replica_binding.replica_id.as_uuid() {
            return Err(FlowError::Runtime(
                "Runtime cleanup changed its replica aggregate".into(),
            ));
        }
        return Ok(CleanupDispatchStepOutput::Ready {
            dispatched: DispatchedCleanup {
                node_id,
                command_id,
                result_deadline: action_result_deadline(action, &command, &input.resolved.spec)?,
                cleanup_deadline,
                attempt: input.attempt,
            },
        });
    }

    let payload = action.payload(
        deployment.id,
        input.attempt,
        &input.resolved.spec,
        runtime_deadline,
    )?;
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: replica_binding.replica_id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue Runtime cleanup", error))?
        .value;
    if command.id != command_id
        || command.node_id != node_id
        || command.aggregate_id != replica_binding.replica_id.as_uuid()
    {
        return Err(FlowError::Runtime(
            "node command repository changed the cleanup command identity".into(),
        ));
    }
    deployment = if deployment.status == DeploymentStatus::Cancelling {
        runtime
            .workloads
            .begin_cleanup(deployment.id, deployment.aggregate_version, command_id, now)
            .await
    } else {
        runtime
            .workloads
            .retry_cleanup(deployment.id, deployment.aggregate_version, command_id, now)
            .await
    }
    .map_err(|error| flow_error("could not persist Runtime cleanup dispatch", error))?;
    Ok(CleanupDispatchStepOutput::Ready {
        dispatched: DispatchedCleanup {
            node_id: deployment
                .node_id
                .ok_or_else(|| FlowError::Runtime("cleanup deployment omitted its node".into()))?,
            command_id: deployment.cleanup_command_id.ok_or_else(|| {
                FlowError::Runtime("cleanup deployment omitted its command".into())
            })?,
            result_deadline,
            cleanup_deadline,
            attempt: input.attempt,
        },
    })
}

pub(super) async fn observe_cleanup(
    runtime: &DeploymentFlowRuntime,
    input: CleanupObserveStepInput,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    observe(runtime, input, CleanupAction::Stop).await
}

pub(super) async fn observe_removal(
    runtime: &DeploymentFlowRuntime,
    input: CleanupObserveStepInput,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    observe(runtime, input, CleanupAction::Remove).await
}

async fn observe(
    runtime: &DeploymentFlowRuntime,
    input: CleanupObserveStepInput,
    action: CleanupAction,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for cleanup observation", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.status == DeploymentStatus::Cancelled {
        return Ok(CleanupObserveStepOutput::Ready {
            cleaned_at: deployment.cancelled_at.unwrap_or(deployment.updated_at),
        });
    }
    if deployment.status != DeploymentStatus::CleanupPending
        || deployment.node_id != Some(input.dispatched.node_id)
        || deployment.cleanup_command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "deployment cleanup observation identity does not match dispatch".into(),
        ));
    }

    if action == CleanupAction::Stop {
        if let Some(record) = runtime
            .node_control
            .latest_runtime_observation(
                input.dispatched.node_id,
                &input.resolved.spec.unit_id,
                input.resolved.spec.generation,
            )
            .await
            .map_err(|error| flow_error("could not load Runtime cleanup observation", error))?
        {
            if record.command_id == Some(input.dispatched.command_id)
                && record.observation.state == RuntimeUnitState::Stopped
            {
                return Ok(CleanupObserveStepOutput::Ready {
                    cleaned_at: record.received_at,
                });
            }
        }
    }

    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load Runtime cleanup result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                if action.result_matches(result.as_ref(), &input.resolved.spec) {
                    return Ok(CleanupObserveStepOutput::Ready {
                        cleaned_at: acknowledgement.completed_at,
                    });
                }
                return Ok(CleanupObserveStepOutput::Failed {
                    reason: format!(
                        "Runtime {} completed without authoritative cleanup evidence",
                        action.name()
                    ),
                });
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                let now = Utc::now();
                if failure.retryable && now < input.dispatched.cleanup_deadline {
                    return Ok(CleanupObserveStepOutput::Retry {
                        reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                        next_attempt_at: now,
                        deadline_at: input.dispatched.cleanup_deadline,
                    });
                }
                return Ok(CleanupObserveStepOutput::Failed {
                    reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                });
            }
        }
    }

    let now = Utc::now();
    if now >= input.dispatched.cleanup_deadline {
        return Ok(CleanupObserveStepOutput::Failed {
            reason: "Runtime cleanup did not complete before its independent deadline".into(),
        });
    }
    if now >= input.dispatched.result_deadline {
        return Ok(CleanupObserveStepOutput::Retry {
            reason: format!(
                "Runtime {} attempt did not produce durable evidence before its deadline",
                action.name()
            ),
            next_attempt_at: now,
            deadline_at: input.dispatched.cleanup_deadline,
        });
    }
    Ok(CleanupObserveStepOutput::Pending {
        reason: format!(
            "waiting for authoritative Runtime {} evidence",
            action.name()
        ),
        next_poll_at: next_poll(
            now,
            runtime.config.cleanup_poll,
            input
                .dispatched
                .result_deadline
                .min(input.dispatched.cleanup_deadline),
        )?,
        deadline_at: input
            .dispatched
            .result_deadline
            .min(input.dispatched.cleanup_deadline),
    })
}

pub(super) async fn dispatch_failed(
    runtime: &DeploymentFlowRuntime,
    input: CleanupDispatchStepInput,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| {
            flow_error("could not load failed candidate for Runtime cleanup", error)
        })?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if !matches!(
        deployment.status,
        DeploymentStatus::Resolving
            | DeploymentStatus::Scheduled
            | DeploymentStatus::Applying
            | DeploymentStatus::Verifying
    ) {
        return Err(FlowError::Runtime(format!(
            "failed candidate cannot clean up from {}",
            deployment.status.as_str()
        )));
    }
    let cleanup_deadline = input
        .resolved
        .convergence_deadline
        .checked_add_signed(runtime.config.cleanup_timeout)
        .ok_or_else(|| FlowError::Runtime("failed candidate cleanup deadline overflowed".into()))?;
    let now = Utc::now().max(deployment.updated_at);
    if now >= cleanup_deadline {
        return Ok(CleanupDispatchStepOutput::Failed {
            reason: "failed candidate Runtime was not fenced before its cleanup deadline".into(),
        });
    }
    if deployment.command_id.is_none() {
        return Ok(CleanupDispatchStepOutput::NotRequired { cleaned_at: now });
    }
    let replica_binding = runtime
        .workloads
        .find_deployment_replica_binding(deployment.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not load failed candidate replica binding", error))?;
    validate_resolved_replica_binding(&replica_binding, &deployment, &input.resolved)?;
    let node_id = deployment
        .node_id
        .ok_or_else(|| FlowError::Runtime("failed candidate omitted its Runtime node".into()))?;
    let issued_at = input.issued_at.unwrap_or(now).max(deployment.updated_at);
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("failed cleanup command deadline overflowed".into()))?
        .min(cleanup_deadline);
    let runtime_deadline = issued_at
        .checked_add_signed(runtime.config.runtime_stop_timeout)
        .ok_or_else(|| FlowError::Runtime("failed Runtime stop deadline overflowed".into()))?
        .min(cleanup_deadline);
    let result_deadline = not_after.min(runtime_deadline);
    if now >= result_deadline {
        return Ok(CleanupDispatchStepOutput::Retry {
            reason: "failed Runtime cleanup attempt expired before dispatch".into(),
            next_attempt_at: now,
            deadline_at: cleanup_deadline,
        });
    }
    let command_id = failed_cleanup_command_id(deployment.id, input.attempt);
    let payload = NodeCommandPayload::RuntimeStop {
        request: RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("deployment:{}:failed-stop:{}", deployment.id, input.attempt),
            unit_id: input.resolved.spec.unit_id.clone(),
            generation: input.resolved.spec.generation,
            deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
        },
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: replica_binding.replica_id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue failed Runtime cleanup", error))?
        .value;
    if command.id != command_id
        || command.node_id != node_id
        || command.aggregate_id != replica_binding.replica_id.as_uuid()
    {
        return Err(FlowError::Runtime(
            "failed Runtime cleanup command identity changed".into(),
        ));
    }
    let result_deadline = stop_result_deadline(&command, &input.resolved.spec)?;
    Ok(CleanupDispatchStepOutput::Ready {
        dispatched: DispatchedCleanup {
            node_id,
            command_id,
            result_deadline,
            cleanup_deadline,
            attempt: input.attempt,
        },
    })
}

pub(super) async fn observe_failed(
    runtime: &DeploymentFlowRuntime,
    input: CleanupObserveStepInput,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| {
            flow_error("could not load failed candidate cleanup observation", error)
        })?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if !matches!(
        deployment.status,
        DeploymentStatus::Applying | DeploymentStatus::Verifying
    ) || deployment.node_id != Some(input.dispatched.node_id)
    {
        return Err(FlowError::Runtime(
            "failed candidate cleanup observation changed its Runtime identity".into(),
        ));
    }

    if let Some(record) = runtime
        .node_control
        .latest_runtime_observation(
            input.dispatched.node_id,
            &input.resolved.spec.unit_id,
            input.resolved.spec.generation,
        )
        .await
        .map_err(|error| flow_error("could not load failed Runtime cleanup observation", error))?
    {
        if record.command_id == Some(input.dispatched.command_id)
            && record.observation.state == RuntimeUnitState::Stopped
        {
            return Ok(CleanupObserveStepOutput::Ready {
                cleaned_at: record.received_at,
            });
        }
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load failed Runtime cleanup acknowledgement",
                error,
            )
        })?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => {
                match result.as_ref() {
                    a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                        inspection: RuntimeInspection::NotFound { .. },
                    } => {
                        return Ok(CleanupObserveStepOutput::Ready {
                            cleaned_at: acknowledgement.completed_at,
                        })
                    }
                    a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                        inspection: RuntimeInspection::Found { observation, .. },
                    } if observation.state == RuntimeUnitState::Stopped => {
                        return Ok(CleanupObserveStepOutput::Ready {
                            cleaned_at: acknowledgement.completed_at,
                        })
                    }
                    _ => return Ok(CleanupObserveStepOutput::Failed {
                        reason:
                            "failed Runtime cleanup completed without stopped or absent evidence"
                                .into(),
                    }),
                }
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                let now = Utc::now();
                let reason = bounded_reason(format!("{}: {}", failure.code, failure.message));
                if (failure.retryable
                    || matches!(
                        failure.code.as_str(),
                        "command_expired" | "stale_generation"
                    ))
                    && now < input.dispatched.cleanup_deadline
                {
                    return Ok(CleanupObserveStepOutput::Retry {
                        reason,
                        next_attempt_at: now,
                        deadline_at: input.dispatched.cleanup_deadline,
                    });
                }
                return Ok(CleanupObserveStepOutput::Failed { reason });
            }
        }
    }
    let now = Utc::now();
    if now >= input.dispatched.cleanup_deadline {
        return Ok(CleanupObserveStepOutput::Failed {
            reason: "failed candidate Runtime was not fenced before its cleanup deadline".into(),
        });
    }
    if now >= input.dispatched.result_deadline {
        return Ok(CleanupObserveStepOutput::Retry {
            reason: "failed Runtime cleanup produced no durable fencing evidence".into(),
            next_attempt_at: now,
            deadline_at: input.dispatched.cleanup_deadline,
        });
    }
    let deadline_at = input
        .dispatched
        .result_deadline
        .min(input.dispatched.cleanup_deadline);
    Ok(CleanupObserveStepOutput::Pending {
        reason: "waiting for failed Runtime stopped or absent evidence".into(),
        next_poll_at: next_poll(now, runtime.config.cleanup_poll, deadline_at)?,
        deadline_at,
    })
}

pub(super) async fn complete_cancellation(
    runtime: &DeploymentFlowRuntime,
    input: CompleteCancellationStepInput,
) -> a3s_flow::Result<CompleteCancellationStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.organization_id, input.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment cancellation", error))?;
    cancel_database_reservation(
        runtime,
        input.organization_id,
        deployment.id,
        input.cleaned_at,
    )
    .await?;
    let cancelled = runtime
        .workloads
        .cancel(
            deployment.id,
            deployment.aggregate_version,
            input.cleaned_at.max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not complete deployment cancellation", error))?;
    Ok(CompleteCancellationStepOutput {
        deployment_id: cancelled.id,
        cancelled_at: cancelled.cancelled_at.ok_or_else(|| {
            FlowError::Runtime("cancelled deployment omitted its completion time".into())
        })?,
        operation_status: "cancelled".into(),
    })
}

fn stop_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
    expected_spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<DateTime<Utc>> {
    action_result_deadline(CleanupAction::Stop, command, expected_spec)
}

fn action_result_deadline(
    action: CleanupAction,
    command: &crate::modules::fleet::domain::entities::NodeCommand,
    expected_spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<DateTime<Utc>> {
    let request = match (action, &command.payload) {
        (CleanupAction::Stop, NodeCommandPayload::RuntimeStop { request })
        | (CleanupAction::Remove, NodeCommandPayload::RuntimeRemove { request }) => request,
        _ => {
            return Err(FlowError::Runtime(format!(
                "deployment cleanup command is not a Runtime {} request",
                action.name()
            )))
        }
    };
    if request.unit_id != expected_spec.unit_id || request.generation != expected_spec.generation {
        return Err(FlowError::Runtime(
            "deployment cleanup command changed its Runtime identity".into(),
        ));
    }
    let deadline_ms = request.deadline_at_ms.ok_or_else(|| {
        FlowError::Runtime(format!(
            "Runtime {} command omitted its deadline",
            action.name()
        ))
    })?;
    let deadline_ms = i64::try_from(deadline_ms).map_err(|_| {
        FlowError::Runtime(format!(
            "Runtime {} deadline exceeds supported range",
            action.name()
        ))
    })?;
    DateTime::from_timestamp_millis(deadline_ms)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime(format!("Runtime {} deadline is invalid", action.name())))
}

fn failed_cleanup_command_id(
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &deployment_id.as_uuid(),
        format!("failed-runtime-stop:{attempt}").as_bytes(),
    ))
}
