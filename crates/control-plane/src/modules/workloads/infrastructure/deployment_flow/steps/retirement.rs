use super::super::types::{
    ActivateStepOutput, CompleteRetirementStepInput, DispatchedRetirement,
    RetirementDispatchStepInput, RetirementDispatchStepOutput, RetirementObserveStepInput,
    RetirementObserveStepOutput,
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

pub(super) async fn dispatch(
    runtime: &DeploymentFlowRuntime,
    input: RetirementDispatchStepInput,
) -> a3s_flow::Result<RetirementDispatchStepOutput> {
    let previous = input.resolved.previous_runtime.as_ref().ok_or_else(|| {
        FlowError::Runtime("Runtime retirement requires a previous revision".into())
    })?;
    let ActivateStepOutput::Active {
        deployment_id,
        workload_id,
        revision_id,
        activated_at,
        ..
    } = input.activation
    else {
        return Err(FlowError::Runtime(
            "Runtime retirement requires an activated deployment".into(),
        ));
    };
    let mut deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment for Runtime retirement", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.id != deployment_id
        || deployment.workload_id != workload_id
        || deployment.revision_id != revision_id
    {
        return Err(FlowError::Runtime(
            "Runtime retirement activation identity changed".into(),
        ));
    }
    if deployment.status == DeploymentStatus::Active {
        return Ok(RetirementDispatchStepOutput::NotRequired {
            retired_at: deployment.updated_at,
        });
    }
    if deployment.status != DeploymentStatus::Retiring {
        return Err(FlowError::Runtime(format!(
            "deployment cannot retire its previous Runtime from {}",
            deployment.status.as_str()
        )));
    }
    if deployment.node_id != Some(previous.node_id) {
        return Err(FlowError::Runtime(
            "one-node update changed nodes before Runtime retirement".into(),
        ));
    }
    let retirement_deadline = activated_at
        .checked_add_signed(runtime.config.cleanup_timeout)
        .ok_or_else(|| FlowError::Runtime("Runtime retirement deadline overflowed".into()))?;
    let now = Utc::now().max(deployment.updated_at);
    let issued_at = input.issued_at.unwrap_or(activated_at);
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("retirement command deadline overflowed".into()))?;
    let runtime_deadline = issued_at
        .checked_add_signed(runtime.config.runtime_stop_timeout)
        .ok_or_else(|| FlowError::Runtime("Runtime retirement stop deadline overflowed".into()))?;
    let result_deadline = retirement_deadline.min(not_after).min(runtime_deadline);
    let command_id = retirement_command_id(deployment.id, input.attempt);
    let payload = NodeCommandPayload::RuntimeStop {
        request: RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("deployment:{}:retire:{}", deployment.id, input.attempt),
            unit_id: previous.spec.unit_id.clone(),
            generation: previous.spec.generation,
            deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
        },
    };
    let existing = runtime
        .node_control
        .find_command(previous.node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload Runtime retirement command", error))?;
    if existing.is_none() {
        if deployment.retirement_command_id == Some(command_id) {
            return Err(FlowError::Runtime(
                "persisted Runtime retirement command is missing".into(),
            ));
        }
        if now >= retirement_deadline {
            return Ok(RetirementDispatchStepOutput::Failed {
                reason: "previous Runtime revision was not retired before its deadline".into(),
            });
        }
        if now >= result_deadline {
            return Ok(RetirementDispatchStepOutput::Retry {
                reason: "retirement attempt expired before Runtime stop dispatch".into(),
                next_attempt_at: now,
                deadline_at: retirement_deadline,
            });
        }
    }
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: previous.node_id,
            aggregate_id: deployment.workload_id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: deployment.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue Runtime retirement", error))?
        .value;
    if command.id != command_id || command.node_id != previous.node_id {
        return Err(FlowError::Runtime(
            "node command repository changed the retirement command identity".into(),
        ));
    }
    if deployment.retirement_command_id != Some(command_id) {
        deployment = runtime
            .workloads
            .dispatch_retirement(deployment.id, deployment.aggregate_version, command_id, now)
            .await
            .map_err(|error| flow_error("could not persist Runtime retirement dispatch", error))?;
    }
    Ok(RetirementDispatchStepOutput::Ready {
        dispatched: DispatchedRetirement {
            node_id: previous.node_id,
            command_id: deployment.retirement_command_id.ok_or_else(|| {
                FlowError::Runtime("retiring deployment omitted its command".into())
            })?,
            result_deadline: stop_result_deadline(&command, &previous.spec)?,
            retirement_deadline,
            attempt: input.attempt,
        },
    })
}

pub(super) async fn observe(
    runtime: &DeploymentFlowRuntime,
    input: RetirementObserveStepInput,
) -> a3s_flow::Result<RetirementObserveStepOutput> {
    let previous = input.resolved.previous_runtime.as_ref().ok_or_else(|| {
        FlowError::Runtime("Runtime retirement requires a previous revision".into())
    })?;
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| {
            flow_error(
                "could not load deployment for Runtime retirement observation",
                error,
            )
        })?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.status == DeploymentStatus::Active {
        return Ok(RetirementObserveStepOutput::Ready {
            retired_at: deployment.updated_at,
        });
    }
    if deployment.status != DeploymentStatus::Retiring
        || previous.node_id != input.dispatched.node_id
        || deployment.retirement_command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "Runtime retirement observation identity does not match dispatch".into(),
        ));
    }

    if let Some(record) = runtime
        .node_control
        .latest_runtime_observation(
            input.dispatched.node_id,
            &previous.spec.unit_id,
            previous.spec.generation,
        )
        .await
        .map_err(|error| flow_error("could not load Runtime retirement observation", error))?
    {
        if record.command_id == Some(input.dispatched.command_id)
            && record.observation.state == RuntimeUnitState::Stopped
        {
            return Ok(RetirementObserveStepOutput::Ready {
                retired_at: record.received_at,
            });
        }
    }

    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load Runtime retirement result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::NotFound { .. },
                } => {
                    return Ok(RetirementObserveStepOutput::Ready {
                        retired_at: acknowledgement.completed_at,
                    })
                }
                a3s_cloud_contracts::NodeCommandResult::RuntimeStopped {
                    inspection: RuntimeInspection::Found { observation, .. },
                } if observation.state == RuntimeUnitState::Stopped => {
                    return Ok(RetirementObserveStepOutput::Ready {
                        retired_at: acknowledgement.completed_at,
                    })
                }
                _ => {
                    return Ok(RetirementObserveStepOutput::Failed {
                        reason: "Runtime retirement completed without stopped or absent evidence"
                            .into(),
                    })
                }
            },
            NodeCommandOutcome::Rejected { failure }
                if matches!(failure.code.as_str(), "not_found" | "stale_generation") =>
            {
                return Ok(RetirementObserveStepOutput::Ready {
                    retired_at: acknowledgement.completed_at,
                })
            }
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                let now = Utc::now();
                if failure.retryable && now < input.dispatched.retirement_deadline {
                    return Ok(RetirementObserveStepOutput::Retry {
                        reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                        next_attempt_at: now,
                        deadline_at: input.dispatched.retirement_deadline,
                    });
                }
                return Ok(RetirementObserveStepOutput::Failed {
                    reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                });
            }
        }
    }

    let now = Utc::now();
    if now >= input.dispatched.retirement_deadline {
        return Ok(RetirementObserveStepOutput::Failed {
            reason: "previous Runtime revision was not retired before its deadline".into(),
        });
    }
    if now >= input.dispatched.result_deadline {
        return Ok(RetirementObserveStepOutput::Retry {
            reason: "Runtime retirement did not produce durable evidence before its deadline"
                .into(),
            next_attempt_at: now,
            deadline_at: input.dispatched.retirement_deadline,
        });
    }
    let deadline = input
        .dispatched
        .result_deadline
        .min(input.dispatched.retirement_deadline);
    Ok(RetirementObserveStepOutput::Pending {
        reason: "waiting for stopped or absent previous Runtime evidence".into(),
        next_poll_at: next_poll(now, runtime.config.cleanup_poll, deadline)?,
        deadline_at: deadline,
    })
}

pub(super) async fn complete(
    runtime: &DeploymentFlowRuntime,
    input: CompleteRetirementStepInput,
) -> a3s_flow::Result<ActivateStepOutput> {
    let ActivateStepOutput::Active {
        deployment_id,
        workload_id,
        revision_id,
        activated_at,
        ..
    } = input.activation
    else {
        return Err(FlowError::Runtime(
            "Runtime retirement completion requires activation".into(),
        ));
    };
    let deployment = runtime
        .workloads
        .find_deployment(input.resolved.organization_id, input.resolved.deployment_id)
        .await
        .map_err(|error| flow_error("could not load deployment retirement", error))?;
    validate_resolved_deployment(&input.resolved, &deployment)?;
    if deployment.id != deployment_id
        || deployment.workload_id != workload_id
        || deployment.revision_id != revision_id
    {
        return Err(FlowError::Runtime(
            "Runtime retirement completion changed activation identity".into(),
        ));
    }
    let active = runtime
        .workloads
        .complete_retirement(
            deployment.id,
            deployment.aggregate_version,
            input.retired_at.max(deployment.updated_at),
        )
        .await
        .map_err(|error| flow_error("could not complete Runtime retirement", error))?;
    Ok(ActivateStepOutput::Active {
        deployment_id: active.id,
        workload_id: active.workload_id,
        revision_id: active.revision_id,
        activated_at,
        retired_at: Some(active.updated_at),
    })
}

fn stop_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
    expected_spec: &a3s_runtime::contract::RuntimeUnitSpec,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeStop { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "deployment retirement command is not a Runtime stop request".into(),
        ));
    };
    if request.unit_id != expected_spec.unit_id || request.generation != expected_spec.generation {
        return Err(FlowError::Runtime(
            "deployment retirement command changed its Runtime identity".into(),
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

fn retirement_command_id(
    deployment_id: crate::modules::shared_kernel::domain::DeploymentId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &deployment_id.as_uuid(),
        format!("runtime-retire:{attempt}").as_bytes(),
    ))
}
