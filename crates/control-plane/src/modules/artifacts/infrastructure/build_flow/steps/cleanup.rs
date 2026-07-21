use super::super::types::{
    CleanupDispatchStepInput, CleanupDispatchStepOutput, CleanupObserveStepInput,
    CleanupObserveStepOutput, DispatchedCleanup,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{load_build, load_revision, next_poll, project_spec, timestamp_millis};
use crate::modules::artifacts::domain::BuildRunStatus;
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeCommandResult};
use a3s_flow::FlowError;
use a3s_runtime::contract::RuntimeActionRequest;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn dispatch(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: CleanupDispatchStepInput,
) -> a3s_flow::Result<CleanupDispatchStepOutput> {
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
        BuildRunStatus::Validating | BuildRunStatus::Cancelling | BuildRunStatus::CleanupPending
    ) {
        return Err(FlowError::Runtime(format!(
            "build cannot clean up from {}",
            build.status.as_str()
        )));
    }
    let revision = load_revision(runtime, &build).await?;
    let spec = project_spec(runtime, &build, &revision)?;
    let node_id = build
        .node_id
        .ok_or_else(|| FlowError::Runtime("dispatched build omitted its Runtime node".into()))?;
    let first_issued_at = build.updated_at;
    let cleanup_deadline = input.cleanup_deadline.unwrap_or(
        first_issued_at
            .checked_add_signed(runtime.config.cleanup_timeout)
            .ok_or_else(|| FlowError::Runtime("build cleanup deadline overflowed".into()))?,
    );
    let issued_at = input.issued_at.unwrap_or(first_issued_at);
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("build cleanup command deadline overflowed".into()))?;
    let result_deadline = not_after.min(cleanup_deadline);
    let now = Utc::now().max(build.updated_at);
    if now >= cleanup_deadline {
        return Err(FlowError::Runtime(
            "build Runtime cleanup exceeded its independent deadline".into(),
        ));
    }
    if now >= result_deadline {
        return Ok(CleanupDispatchStepOutput::Retry {
            reason: "build cleanup command expired before dispatch".into(),
            next_attempt_at: now,
            deadline_at: cleanup_deadline,
        });
    }
    let command_id = cleanup_command_id(build.id, input.attempt);
    if build.cleanup_command_id == Some(command_id) {
        let command = runtime
            .node_control
            .find_command(node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload build cleanup command", error))?
            .ok_or_else(|| FlowError::Runtime("build cleanup command is missing".into()))?;
        validate_remove_command(&build, &spec, input.attempt, &command)?;
        return Ok(CleanupDispatchStepOutput::Ready {
            dispatched: DispatchedCleanup {
                node_id,
                command_id,
                result_deadline: remove_result_deadline(&command)?.min(cleanup_deadline),
                cleanup_deadline,
                attempt: input.attempt,
            },
        });
    }
    let payload = NodeCommandPayload::RuntimeRemove {
        request: RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("build:{}:remove:{}", build.id, input.attempt),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            deadline_at_ms: Some(timestamp_millis(result_deadline)?),
        },
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id,
            aggregate_id: build.id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: build.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue build cleanup command", error))?
        .value;
    validate_remove_command(&build, &spec, input.attempt, &command)?;
    let expected = build.aggregate_version;
    if build.cleanup_command_id.is_some() {
        build
            .retry_cleanup(command_id, now)
            .map_err(|error| flow_error("could not retry build Runtime cleanup", error))?;
    } else {
        build
            .begin_cleanup(command_id, now)
            .map_err(|error| flow_error("could not begin build Runtime cleanup", error))?;
    }
    let cleanup = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist build Runtime cleanup", error))?;
    Ok(CleanupDispatchStepOutput::Ready {
        dispatched: DispatchedCleanup {
            node_id,
            command_id: cleanup.cleanup_command_id.ok_or_else(|| {
                FlowError::Runtime("build cleanup omitted its Runtime command".into())
            })?,
            result_deadline,
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
            "build cleanup observation changed its durable identity".into(),
        ));
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(input.dispatched.node_id, input.dispatched.command_id)
        .await
        .map_err(|error| flow_error("could not load build cleanup result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::RuntimeRemoved { removal }
                    if removal.unit_id == format!("cloud-build-{}", input.flow.build_run_id)
                        && removal.generation == 1 =>
                {
                    return Ok(CleanupObserveStepOutput::Ready {
                        cleaned_at: acknowledgement.completed_at,
                    })
                }
                _ => {
                    return retry(
                        runtime,
                        "build cleanup completed without exact Runtime removal evidence",
                        input.dispatched.cleanup_deadline,
                    )
                }
            },
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return retry(
                    runtime,
                    &format!("{}: {}", failure.code, failure.message),
                    input.dispatched.cleanup_deadline,
                )
            }
        }
    }
    let now = Utc::now();
    if now >= input.dispatched.result_deadline {
        return retry(
            runtime,
            "build cleanup command did not finish before its attempt deadline",
            input.dispatched.cleanup_deadline,
        );
    }
    Ok(CleanupObserveStepOutput::Pending {
        reason: "waiting for build Runtime removal evidence".into(),
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.dispatched.result_deadline,
        )?,
        deadline_at: input.dispatched.result_deadline,
    })
}

fn retry(
    runtime: &BuildFlowRuntime,
    reason: &str,
    deadline: DateTime<Utc>,
) -> a3s_flow::Result<CleanupObserveStepOutput> {
    let now = Utc::now();
    if now >= deadline {
        return Err(FlowError::Runtime(
            "build Runtime cleanup exceeded its independent deadline".into(),
        ));
    }
    Ok(CleanupObserveStepOutput::Retry {
        reason: super::common::bounded_reason(reason),
        next_attempt_at: next_poll(now, runtime.config.observation_poll, deadline)?,
        deadline_at: deadline,
    })
}

fn cleanup_command_id(
    build_id: crate::modules::shared_kernel::domain::BuildRunId,
    attempt: u32,
) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &build_id.as_uuid(),
        format!("runtime-remove:{attempt}").as_bytes(),
    ))
}

fn validate_remove_command(
    build: &crate::modules::artifacts::domain::BuildRun,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    attempt: u32,
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "build cleanup command is not a Runtime remove".into(),
        ));
    };
    if command.id != cleanup_command_id(build.id, attempt)
        || command.node_id
            != build.node_id.ok_or_else(|| {
                FlowError::Runtime("dispatched build omitted its Runtime node".into())
            })?
        || command.aggregate_id != build.id.as_uuid()
        || command.correlation_id != build.operation_id.as_uuid()
        || request.request_id != format!("build:{}:remove:{attempt}", build.id)
        || request.unit_id != spec.unit_id
        || request.generation != spec.generation
    {
        return Err(FlowError::Runtime(
            "build cleanup command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn remove_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeRemove { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "build cleanup command is not a Runtime remove".into(),
        ));
    };
    let millis = request
        .deadline_at_ms
        .ok_or_else(|| FlowError::Runtime("build Runtime remove omitted its deadline".into()))?;
    let millis = i64::try_from(millis)
        .map_err(|_| FlowError::Runtime("build Runtime remove deadline is invalid".into()))?;
    DateTime::from_timestamp_millis(millis)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("build Runtime remove deadline is invalid".into()))
}
