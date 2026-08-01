use super::super::types::{
    BoxCleanupAction, CleanupDispatchStepInput, CleanupDispatchStepOutput, CleanupObserveStepInput,
    CleanupObserveStepOutput, DispatchedCleanup,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build, load_revision, next_poll, project_request};
use crate::modules::artifacts::domain::BuildRunStatus;
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{BuildRunId, NodeCommandId};
use a3s_cloud_contracts::{
    NodeBoxBuildInspection, NodeBoxBuildRequest, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult,
};
use a3s_flow::FlowError;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn dispatch(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: CleanupDispatchStepInput,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
    if input.attempt == 0 {
        return Err(FlowError::Runtime(
            "Box cleanup command attempt must be positive".into(),
        ));
    }
    let mut build = load_build(runtime, run_id, &input.flow).await?;
    if build.status.is_terminal() {
        return Ok(CleanupDispatchStepOutput::NotRequired {
            cleaned_at: build.finished_at.unwrap_or(build.updated_at),
        });
    }
    if build.command_id.is_none() {
        return Ok(CleanupDispatchStepOutput::NotRequired {
            cleaned_at: Utc::now().max(build.updated_at),
        });
    }
    if !matches!(
        build.status,
        BuildRunStatus::Publishing
            | BuildRunStatus::Attesting
            | BuildRunStatus::Cancelling
            | BuildRunStatus::CleanupPending
    ) {
        return Err(FlowError::Runtime(format!(
            "build cannot clean up from {}",
            build.status.as_str()
        )));
    }

    let revision = load_revision(runtime, &build).await?;
    let request = project_request(runtime, &build, &revision).await?;
    let node_id = build
        .node_id
        .ok_or_else(|| FlowError::Runtime("dispatched build omitted its Box node".into()))?;
    let now = Utc::now().max(build.updated_at);
    let cleanup_deadline = input.cleanup_deadline.unwrap_or(
        build
            .updated_at
            .checked_add_signed(runtime.config.cleanup_timeout)
            .ok_or_else(|| FlowError::Runtime("Box cleanup deadline overflowed".into()))?,
    );
    ensure_before_cleanup_deadline(now, cleanup_deadline)?;
    let issued_at = input.issued_at.unwrap_or(now);
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("Box cleanup command deadline overflowed".into()))?
        .min(cleanup_deadline);
    if now >= not_after {
        return Ok(CleanupDispatchStepOutput::Retry {
            reason: "Box cleanup command expired before dispatch".into(),
            next_attempt_at: next_poll(now, runtime.config.observation_poll, cleanup_deadline)?,
            deadline_at: cleanup_deadline,
        });
    }

    let command_id = cleanup_command_id(build.id, input.action, input.attempt);
    let command = if build.cleanup_command_id == Some(command_id) {
        runtime
            .node_control
            .find_command(node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload Box cleanup command", error))?
            .ok_or_else(|| FlowError::Runtime("Box cleanup command is missing".into()))?
    } else {
        runtime
            .node_control
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: command_id,
                node_id,
                aggregate_id: build.id.as_uuid(),
                payload: cleanup_payload(input.action, &request),
                issued_at,
                not_after,
                correlation_id: build.operation_id.as_uuid(),
            })
            .await
            .map_err(|error| flow_error("could not enqueue Box cleanup command", error))?
            .value
    };
    validate_cleanup_command(&build, &request, input.action, input.attempt, &command)?;

    if build.cleanup_command_id != Some(command_id) {
        let expected = build.aggregate_version;
        if build.cleanup_command_id.is_some() {
            build
                .retry_cleanup(command_id, now)
                .map_err(|error| flow_error("could not advance Box cleanup", error))?;
        } else {
            build
                .begin_cleanup(command_id, now)
                .map_err(|error| flow_error("could not begin Box cleanup", error))?;
        }
        build = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist Box cleanup", error))?;
    }

    Ok(CleanupDispatchStepOutput::Ready {
        dispatched: DispatchedCleanup {
            action: input.action,
            node_id,
            command_id: build.cleanup_command_id.ok_or_else(|| {
                FlowError::Runtime("Box cleanup command was not persisted".into())
            })?,
            result_deadline: command.not_after.min(cleanup_deadline),
            cleanup_deadline,
            attempt: input.attempt,
        },
    })
}

pub(super) async fn observe(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: CleanupObserveStepInput,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    let build = load_build(runtime, run_id, &input.flow).await?;
    if build.status.is_terminal() {
        return Ok(CleanupObserveStepOutput::Ready {
            cleaned_at: build.finished_at.unwrap_or(build.updated_at),
        });
    }
    if build.status != BuildRunStatus::CleanupPending
        || build.node_id != Some(input.dispatched.node_id)
        || build.cleanup_command_id != Some(input.dispatched.command_id)
    {
        return Err(FlowError::Runtime(
            "Box cleanup observation changed its durable identity".into(),
        ));
    }
    let revision = load_revision(runtime, &build).await?;
    let request = project_request(runtime, &build, &revision).await?;
    let command = runtime
        .node_control
        .find_command(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not reload Box cleanup command", error))?
        .ok_or_else(|| FlowError::Runtime("Box cleanup command is missing".into()))?;
    validate_cleanup_command(
        &build,
        &request,
        input.dispatched.action,
        input.dispatched.attempt,
        &command,
    )?;

    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load Box cleanup result", error))?
    {
        let completed_at = acknowledgement.completed_at.max(build.updated_at);
        return match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match (input.dispatched.action, *result) {
                (BoxCleanupAction::Cancel, NodeCommandResult::BoxBuildCancelled { cancelled }) => {
                    cancelled
                        .validate_for(&request)
                        .map_err(|error| flow_error("Box cancellation result is invalid", error))?;
                    advance(
                        runtime,
                        BoxCleanupAction::Inspect,
                        "Box cancellation is terminally acknowledged",
                        input.dispatched.cleanup_deadline,
                    )
                }
                (
                    BoxCleanupAction::Inspect,
                    NodeCommandResult::BoxBuildInspected { inspection },
                ) => {
                    inspection
                        .validate_for(&request)
                        .map_err(|error| flow_error("Box cleanup inspection is invalid", error))?;
                    let (action, reason) = match inspection.as_ref() {
                        NodeBoxBuildInspection::Running { .. }
                        | NodeBoxBuildInspection::Cancelling { .. } => (
                            BoxCleanupAction::Inspect,
                            "Box build has not reached a terminal phase",
                        ),
                        NodeBoxBuildInspection::Succeeded { .. }
                        | NodeBoxBuildInspection::Cancelled { .. }
                        | NodeBoxBuildInspection::Failed { .. } => (
                            BoxCleanupAction::Remove,
                            "Box build reached a terminal phase",
                        ),
                    };
                    advance(runtime, action, reason, input.dispatched.cleanup_deadline)
                }
                (BoxCleanupAction::Remove, NodeCommandResult::BoxBuildRemoved { removed }) => {
                    removed
                        .validate_for(&request)
                        .map_err(|error| flow_error("Box removal result is invalid", error))?;
                    Ok(CleanupObserveStepOutput::Ready {
                        cleaned_at: completed_at,
                    })
                }
                _ => Err(FlowError::Runtime(
                    "Box cleanup command returned another result kind".into(),
                )),
            },
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                advance(
                    runtime,
                    input.dispatched.action,
                    &format!("{}: {}", failure.code, failure.message),
                    input.dispatched.cleanup_deadline,
                )
            }
        };
    }

    let now = Utc::now().max(build.updated_at);
    ensure_before_cleanup_deadline(now, input.dispatched.cleanup_deadline)?;
    if now >= input.dispatched.result_deadline {
        return advance(
            runtime,
            input.dispatched.action,
            "Box cleanup command did not finish before its attempt deadline",
            input.dispatched.cleanup_deadline,
        );
    }
    Ok(CleanupObserveStepOutput::AwaitingCommand {
        reason: format!(
            "waiting for Box build {} acknowledgement",
            input.dispatched.action.as_str()
        ),
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.dispatched.result_deadline,
        )?,
        deadline_at: input.dispatched.result_deadline,
    })
}

fn advance(
    runtime: &BuildFlowRuntime,
    action: BoxCleanupAction,
    reason: &str,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    let now = Utc::now();
    ensure_before_cleanup_deadline(now, deadline)?;
    Ok(CleanupObserveStepOutput::Advance {
        action,
        reason: bounded_reason(reason),
        next_attempt_at: next_poll(now, runtime.config.observation_poll, deadline)?,
        deadline_at: deadline,
    })
}

fn ensure_before_cleanup_deadline(
    now: DateTime<Utc>,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<()> {
    if now >= deadline {
        Err(FlowError::Runtime(
            "Box cleanup exceeded its independent deadline".into(),
        ))
    } else {
        Ok(())
    }
}

fn cleanup_payload(action: BoxCleanupAction, request: &NodeBoxBuildRequest) -> NodeCommandPayload {
    match action {
        BoxCleanupAction::Cancel => NodeCommandPayload::BoxBuildCancel {
            request: Box::new(request.clone()),
        },
        BoxCleanupAction::Inspect => NodeCommandPayload::BoxBuildInspect {
            request: Box::new(request.clone()),
        },
        BoxCleanupAction::Remove => NodeCommandPayload::BoxBuildRemove {
            request: Box::new(request.clone()),
        },
    }
}

fn cleanup_command_id(
    build_id: BuildRunId,
    action: BoxCleanupAction,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &build_id.as_uuid(),
        format!("box-build-cleanup:{}:{attempt}", action.as_str()).as_bytes(),
    ))
}

fn validate_cleanup_command(
    build: &crate::modules::artifacts::domain::BuildRun,
    request: &NodeBoxBuildRequest,
    action: BoxCleanupAction,
    attempt: u32,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    let admitted = match (action, &command.payload) {
        (BoxCleanupAction::Cancel, NodeCommandPayload::BoxBuildCancel { request })
        | (BoxCleanupAction::Inspect, NodeCommandPayload::BoxBuildInspect { request })
        | (BoxCleanupAction::Remove, NodeCommandPayload::BoxBuildRemove { request }) => {
            request.as_ref()
        }
        _ => {
            return Err(FlowError::Runtime(
                "Box cleanup command action changed during replay".into(),
            ))
        }
    };
    if command.id != cleanup_command_id(build.id, action, attempt)
        || command.node_id
            != build
                .node_id
                .ok_or_else(|| FlowError::Runtime("dispatched build omitted its Box node".into()))?
        || command.aggregate_id != build.id.as_uuid()
        || command.correlation_id != build.operation_id.as_uuid()
        || admitted != request
    {
        return Err(FlowError::Runtime(
            "Box cleanup command changed its durable identity".into(),
        ));
    }
    Ok(())
}
