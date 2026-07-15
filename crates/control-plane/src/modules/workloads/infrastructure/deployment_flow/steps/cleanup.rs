use super::super::types::{
    CleanupDispatchStepInput, CleanupDispatchStepOutput, CleanupObserveStepInput,
    CleanupObserveStepOutput, CompleteCancellationStepInput, CompleteCancellationStepOutput,
    DispatchedCleanup,
};
use super::super::{flow_error, DeploymentFlowRuntime};
use super::{bounded_reason, next_poll, timestamp_millis, validate_resolved_deployment};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use crate::modules::workloads::domain::entities::DeploymentStatus;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_flow::FlowError;
use a3s_runtime::contract::{RuntimeActionRequest, RuntimeInspection, RuntimeUnitState};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn dispatch_cleanup(
    runtime: &DeploymentFlowRuntime,
    input: CleanupDispatchStepInput,
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
            reason: "cleanup attempt expired before Runtime stop dispatch".into(),
            next_attempt_at: now,
            deadline_at: cleanup_deadline,
        });
    }

    let command_id = cleanup_command_id(deployment.id, input.attempt);
    if deployment.cleanup_command_id == Some(command_id) {
        let command = runtime
            .node_control
            .find_command(node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload Runtime cleanup command", error))?
            .ok_or_else(|| FlowError::Runtime("Runtime cleanup command is missing".into()))?;
        return Ok(CleanupDispatchStepOutput::Ready {
            dispatched: DispatchedCleanup {
                node_id,
                command_id,
                result_deadline: stop_result_deadline(&command, &input.resolved.spec)?,
                cleanup_deadline,
                attempt: input.attempt,
            },
        });
    }

    let payload = NodeCommandPayload::RuntimeStop {
        request: RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("deployment:{}:stop:{}", deployment.id, input.attempt),
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
            aggregate_id: deployment.workload_id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue Runtime cleanup", error))?
        .value;
    if command.id != command_id || command.node_id != node_id {
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

    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load Runtime cleanup result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::NotFound { .. },
                } => {
                    return Ok(CleanupObserveStepOutput::Ready {
                        cleaned_at: acknowledgement.completed_at,
                    })
                }
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::Found { observation },
                } if observation.state == RuntimeUnitState::Stopped => {
                    return Ok(CleanupObserveStepOutput::Ready {
                        cleaned_at: acknowledgement.completed_at,
                    })
                }
                _ => {
                    return Ok(CleanupObserveStepOutput::Failed {
                        reason: "Runtime stop completed without stopped or absent evidence".into(),
                    })
                }
            },
            NodeCommandOutcome::Rejected { failure }
                if matches!(failure.code.as_str(), "not_found" | "stale_generation") =>
            {
                return Ok(CleanupObserveStepOutput::Ready {
                    cleaned_at: acknowledgement.completed_at,
                })
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
            reason: "Runtime stop attempt did not produce durable evidence before its deadline"
                .into(),
            next_attempt_at: now,
            deadline_at: input.dispatched.cleanup_deadline,
        });
    }
    Ok(CleanupObserveStepOutput::Pending {
        reason: "waiting for stopped or absent Runtime evidence".into(),
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

pub(super) async fn complete_cancellation(
    runtime: &DeploymentFlowRuntime,
    input: CompleteCancellationStepInput,
) -> a3s_flow::Result<CompleteCancellationStepOutput> {
    let deployment = runtime
        .workloads
        .find_deployment(input.organization_id, input.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment cancellation", error))?;
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
    let NodeCommandPayload::RuntimeStop { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "deployment cleanup command is not a Runtime stop request".into(),
        ));
    };
    if request.unit_id != expected_spec.unit_id || request.generation != expected_spec.generation {
        return Err(FlowError::Runtime(
            "deployment cleanup command changed its Runtime identity".into(),
        ));
    }
    let deadline_ms = request
        .deadline_at_ms
        .ok_or_else(|| FlowError::Runtime("Runtime stop command omitted its deadline".into()))?;
    let deadline_ms = i64::try_from(deadline_ms)
        .map_err(|_| FlowError::Runtime("Runtime stop deadline exceeds supported range".into()))?;
    DateTime::from_timestamp_millis(deadline_ms)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("Runtime stop deadline is invalid".into()))
}

fn cleanup_command_id(
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &deployment_id.as_uuid(),
        format!("runtime-stop:{attempt}").as_bytes(),
    ))
}
