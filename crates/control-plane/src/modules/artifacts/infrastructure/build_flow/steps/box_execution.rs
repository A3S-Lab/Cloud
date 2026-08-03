//! Box-owned build scheduling, dispatch, and inspection steps.

use super::super::types::{
    DispatchStepInput, DispatchStepOutput, DispatchedBuild, ObserveStepInput, ObserveStepOutput,
    ScheduleStepInput, ScheduleStepOutput,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{bounded_reason, load_build, load_source, next_poll, project_request};
use crate::modules::artifacts::domain::BuildRunStatus;
use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{BuildRunId, NodeCommandId};
use a3s_cloud_contracts::{
    NodeBoxBuildInspection, NodeBoxBuildPhase, NodeBoxBuildRequest, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult,
};
use a3s_flow::FlowError;
use a3s_runtime::contract::RuntimeCapabilities;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn schedule(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: ScheduleStepInput,
) -> a3s_flow::Result<ScheduleStepOutput> {
    let flow = super::super::types::BuildFlowInput {
        organization_id: input.prepared.organization_id,
        build_run_id: input.prepared.build_run_id,
    };
    let mut build = load_build(runtime, run_id, &flow).await?;
    validate_prepared(&build, &input)?;
    if build.cancellation_requested_at.is_some() {
        return Ok(ScheduleStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(ScheduleStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    let source = load_source(runtime, &build).await?;
    let request = project_request(runtime, &build, &source).await?;
    let request_digest = request
        .binding_digest()
        .map_err(|error| flow_error("could not digest Box build request", error))?;
    if let Some(node_id) = build.node_id {
        if build.build_request_digest.as_deref() != Some(request_digest.as_str()) {
            return Err(FlowError::Runtime(
                "scheduled Box build request changed during replay".into(),
            ));
        }
        return Ok(ScheduleStepOutput::Ready {
            node_id,
            request: Box::new(request),
        });
    }
    if build.status != BuildRunStatus::Prepared {
        return Err(FlowError::Runtime(format!(
            "build cannot schedule from {}",
            build.status.as_str()
        )));
    }
    let now = Utc::now().max(build.updated_at);
    let mut nodes = runtime
        .nodes
        .list(build.organization_id)
        .await
        .map_err(|error| flow_error("could not list Box build nodes", error))?;
    nodes.sort_by_key(|node| node.id);
    for node in nodes {
        if !node.accepts_new_work_at(now, runtime.config.heartbeat_timeout) {
            continue;
        }
        let capabilities = match serde_json::from_value::<RuntimeCapabilities>(
            node.capabilities.document().clone(),
        ) {
            Ok(capabilities) => capabilities,
            Err(error) => {
                tracing::warn!(node_id = %node.id, error = %error, "ignoring invalid Runtime capabilities during Box build scheduling");
                continue;
            }
        };
        if capabilities.provider_id.as_str() != "a3s-box" {
            continue;
        }
        let expected = build.aggregate_version;
        build
            .schedule(node.id, request_digest.clone(), now)
            .map_err(|error| flow_error("could not schedule Box build", error))?;
        let scheduled = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist Box build schedule", error))?;
        return Ok(ScheduleStepOutput::Ready {
            node_id: scheduled
                .node_id
                .ok_or_else(|| FlowError::Runtime("scheduled build omitted its Box node".into()))?,
            request: Box::new(request),
        });
    }
    if now >= input.prepared.convergence_deadline {
        return Ok(ScheduleStepOutput::Failed {
            reason: "no ready A3S Box node accepted the build before its deadline".into(),
        });
    }
    Ok(ScheduleStepOutput::Pending {
        reason: "waiting for a ready A3S Box node".into(),
        next_poll_at: next_poll(
            now,
            runtime.config.observation_poll,
            input.prepared.convergence_deadline,
        )?,
        deadline_at: input.prepared.convergence_deadline,
    })
}

pub(super) async fn dispatch(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: DispatchStepInput,
) -> a3s_flow::Result<DispatchStepOutput> {
    let flow = super::super::types::BuildFlowInput {
        organization_id: input.scheduled.prepared.organization_id,
        build_run_id: input.scheduled.prepared.build_run_id,
    };
    let mut build = load_build(runtime, run_id, &flow).await?;
    validate_scheduled(&build, &input)?;
    if build.cancellation_requested_at.is_some() {
        return Ok(DispatchStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(DispatchStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if let Some(command_id) = build.command_id {
        let command = runtime
            .node_control
            .find_command(input.scheduled.node_id, command_id)
            .await
            .map_err(|error| flow_error("could not reload Box build start command", error))?
            .ok_or_else(|| FlowError::Runtime("Box build start command is missing".into()))?;
        validate_start_command(&build, &input.scheduled.request, &command)?;
        return Ok(DispatchStepOutput::Ready {
            dispatched: Box::new(DispatchedBuild {
                result_deadline: result_deadline(runtime, &command)?
                    .min(input.scheduled.prepared.convergence_deadline),
                scheduled: input.scheduled,
                command_id,
            }),
        });
    }
    if build.status != BuildRunStatus::Scheduled {
        return Err(FlowError::Runtime(format!(
            "build cannot dispatch from {}",
            build.status.as_str()
        )));
    }
    let issued_at = build.updated_at;
    let not_after = issued_at
        .checked_add_signed(runtime.config.command_ttl)
        .ok_or_else(|| FlowError::Runtime("Box build start deadline overflowed".into()))?;
    let result_deadline = issued_at
        .checked_add_signed(runtime.config.execution_timeout)
        .ok_or_else(|| FlowError::Runtime("Box build execution deadline overflowed".into()))?
        .min(not_after)
        .min(input.scheduled.prepared.convergence_deadline);
    if Utc::now() >= result_deadline {
        return Ok(DispatchStepOutput::Failed {
            reason: "Box build expired before dispatch".into(),
        });
    }
    let command_id = start_command_id(build.id);
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: input.scheduled.node_id,
            aggregate_id: build.id.as_uuid(),
            payload: NodeCommandPayload::BoxBuildStart {
                request: Box::new(input.scheduled.request.clone()),
            },
            issued_at,
            not_after,
            correlation_id: build.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue Box build start command", error))?
        .value;
    validate_start_command(&build, &input.scheduled.request, &command)?;
    let expected = build.aggregate_version;
    build
        .dispatch(command.id, Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not mark Box build dispatch", error))?;
    let dispatched = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist Box build dispatch", error))?;
    Ok(DispatchStepOutput::Ready {
        dispatched: Box::new(DispatchedBuild {
            scheduled: input.scheduled,
            command_id: dispatched.command_id.ok_or_else(|| {
                FlowError::Runtime("dispatched build omitted its Box start command".into())
            })?,
            result_deadline,
        }),
    })
}

pub(super) async fn observe(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: ObserveStepInput,
) -> a3s_flow::Result<ObserveStepOutput> {
    if input.attempt == 0 {
        return Err(FlowError::Runtime(
            "Box build inspection attempt must be positive".into(),
        ));
    }
    let flow = super::super::types::BuildFlowInput {
        organization_id: input.dispatched.scheduled.prepared.organization_id,
        build_run_id: input.dispatched.scheduled.prepared.build_run_id,
    };
    let mut build = load_build(runtime, run_id, &flow).await?;
    validate_dispatched(&build, &input)?;
    if build.cancellation_requested_at.is_some() {
        return Ok(ObserveStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(ObserveStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if let Some(output) = &build.box_build_output {
        return Ok(ObserveStepOutput::Succeeded {
            output: Box::new(output.clone()),
            completed_at: build.updated_at,
        });
    }

    let start = runtime
        .node_control
        .command_acknowledgement(
            input.dispatched.scheduled.node_id,
            input.dispatched.command_id,
        )
        .await
        .map_err(|error| flow_error("could not load Box build start result", error))?;
    let Some(start) = start else {
        return pending(
            runtime,
            &input,
            false,
            "waiting for the Box build start command",
        );
    };
    match start.outcome {
        NodeCommandOutcome::Succeeded { result } => match *result {
            NodeCommandResult::BoxBuildStarted { started } => match started.phase {
                NodeBoxBuildPhase::Running | NodeBoxBuildPhase::Succeeded => {}
                NodeBoxBuildPhase::Cancelling => {
                    return Ok(ObserveStepOutput::Failed {
                        reason: "Box build began cancelling without a Cloud cancellation request"
                            .into(),
                    })
                }
                NodeBoxBuildPhase::Cancelled { message }
                | NodeBoxBuildPhase::Failed { message } => {
                    return Ok(ObserveStepOutput::Failed {
                        reason: bounded_reason(message),
                    })
                }
            },
            _ => {
                return Err(FlowError::Runtime(
                    "Box build start command returned another result kind".into(),
                ))
            }
        },
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            return Ok(ObserveStepOutput::Failed {
                reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
            })
        }
    }

    let now = Utc::now().max(build.updated_at);
    if now >= input.dispatched.result_deadline {
        return Ok(ObserveStepOutput::Failed {
            reason: "Box build did not finish before its deadline".into(),
        });
    }
    let command_id = inspect_command_id(build.id, input.attempt);
    let command = match runtime
        .node_control
        .find_command(input.dispatched.scheduled.node_id, command_id)
        .await
        .map_err(|error| flow_error("could not reload Box build inspection command", error))?
    {
        Some(command) => command,
        None => {
            let not_after = now
                .checked_add_signed(runtime.config.command_ttl)
                .ok_or_else(|| FlowError::Runtime("Box inspection deadline overflowed".into()))?
                .min(input.dispatched.result_deadline);
            if not_after <= now {
                return Ok(ObserveStepOutput::Failed {
                    reason: "Box build inspection expired before dispatch".into(),
                });
            }
            runtime
                .node_control
                .enqueue_command(NodeCommandDraft {
                    proposed_command_id: command_id,
                    node_id: input.dispatched.scheduled.node_id,
                    aggregate_id: build.id.as_uuid(),
                    payload: NodeCommandPayload::BoxBuildInspect {
                        request: Box::new(input.dispatched.scheduled.request.clone()),
                    },
                    issued_at: now,
                    not_after,
                    correlation_id: build.operation_id.as_uuid(),
                })
                .await
                .map_err(|error| flow_error("could not enqueue Box build inspection", error))?
                .value
        }
    };
    validate_inspect_command(
        &build,
        &input.dispatched.scheduled.request,
        input.attempt,
        &command,
    )?;
    let acknowledgement = runtime
        .node_control
        .command_acknowledgement(input.dispatched.scheduled.node_id, command_id)
        .await
        .map_err(|error| flow_error("could not load Box build inspection result", error))?;
    let Some(acknowledgement) = acknowledgement else {
        return pending(
            runtime,
            &input,
            false,
            "waiting for the Box build inspection command",
        );
    };
    match acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match *result {
            NodeCommandResult::BoxBuildInspected { inspection } => match *inspection {
                NodeBoxBuildInspection::Running { .. }
                | NodeBoxBuildInspection::Cancelling { .. } => {
                    pending(runtime, &input, true, "Box build is still running")
                }
                NodeBoxBuildInspection::Cancelled { message, .. }
                | NodeBoxBuildInspection::Failed { message, .. } => Ok(ObserveStepOutput::Failed {
                    reason: bounded_reason(message),
                }),
                NodeBoxBuildInspection::Succeeded { output, .. } => {
                    output.validate().map_err(|error| {
                        flow_error("Box build output receipt is invalid", error)
                    })?;
                    let completed_at = acknowledgement.completed_at.max(build.updated_at);
                    let expected = build.aggregate_version;
                    build
                        .begin_validation((*output).clone(), completed_at)
                        .map_err(|error| {
                            flow_error("could not begin Box build output validation", error)
                        })?;
                    runtime
                        .builds
                        .save(build, expected)
                        .await
                        .map_err(|error| {
                            flow_error("could not persist Box build output receipt", error)
                        })?;
                    Ok(ObserveStepOutput::Succeeded {
                        output,
                        completed_at,
                    })
                }
            },
            _ => Err(FlowError::Runtime(
                "Box build inspection command returned another result kind".into(),
            )),
        },
        NodeCommandOutcome::Failed { failure } if failure.retryable => pending(
            runtime,
            &input,
            true,
            &format!("{}: {}", failure.code, failure.message),
        ),
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Ok(ObserveStepOutput::Failed {
                reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
            })
        }
    }
}

fn pending(
    runtime: &BuildFlowRuntime,
    input: &ObserveStepInput,
    inspected: bool,
    reason: &str,
) -> a3s_flow::Result<ObserveStepOutput> {
    let now = Utc::now();
    if now >= input.dispatched.result_deadline {
        return Ok(ObserveStepOutput::Failed {
            reason: "Box build did not finish before its deadline".into(),
        });
    }
    let next_poll_at = next_poll(
        now,
        runtime.config.observation_poll,
        input.dispatched.result_deadline,
    )?;
    if inspected {
        Ok(ObserveStepOutput::Running {
            reason: bounded_reason(reason),
            next_poll_at,
            deadline_at: input.dispatched.result_deadline,
        })
    } else {
        Ok(ObserveStepOutput::AwaitingCommand {
            reason: bounded_reason(reason),
            next_poll_at,
            deadline_at: input.dispatched.result_deadline,
        })
    }
}

fn validate_prepared(
    build: &crate::modules::artifacts::domain::BuildRun,
    input: &ScheduleStepInput,
) -> a3s_flow::Result<()> {
    if build.id != input.prepared.build_run_id
        || build.organization_id != input.prepared.organization_id
        || build.subject != input.prepared.subject
        || build.source_content_digest.as_deref()
            != Some(input.prepared.source_content_digest.as_str())
        || build.input_artifact.as_ref() != Some(&input.prepared.input_artifact)
    {
        return Err(FlowError::Runtime(
            "prepared build step input changed durable build identity".into(),
        ));
    }
    Ok(())
}

fn validate_scheduled(
    build: &crate::modules::artifacts::domain::BuildRun,
    input: &DispatchStepInput,
) -> a3s_flow::Result<()> {
    validate_prepared(
        build,
        &ScheduleStepInput {
            prepared: input.scheduled.prepared.clone(),
        },
    )?;
    let digest = input
        .scheduled
        .request
        .binding_digest()
        .map_err(|error| flow_error("scheduled Box build request is invalid", error))?;
    if build.node_id != Some(input.scheduled.node_id)
        || build.build_request_digest.as_deref() != Some(digest.as_str())
    {
        return Err(FlowError::Runtime(
            "scheduled build step input changed Box identity".into(),
        ));
    }
    Ok(())
}

fn validate_dispatched(
    build: &crate::modules::artifacts::domain::BuildRun,
    input: &ObserveStepInput,
) -> a3s_flow::Result<()> {
    validate_scheduled(
        build,
        &DispatchStepInput {
            scheduled: input.dispatched.scheduled.clone(),
        },
    )?;
    if build.command_id != Some(input.dispatched.command_id) {
        return Err(FlowError::Runtime(
            "dispatched build step input changed Box command identity".into(),
        ));
    }
    Ok(())
}

fn validate_start_command(
    build: &crate::modules::artifacts::domain::BuildRun,
    request: &NodeBoxBuildRequest,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::BoxBuildStart { request: admitted } = &command.payload else {
        return Err(FlowError::Runtime(
            "build start command is not a Box build start".into(),
        ));
    };
    if command.id != start_command_id(build.id)
        || command.node_id
            != build
                .node_id
                .ok_or_else(|| FlowError::Runtime("scheduled build omitted its Box node".into()))?
        || command.aggregate_id != build.id.as_uuid()
        || command.correlation_id != build.operation_id.as_uuid()
        || admitted.as_ref() != request
    {
        return Err(FlowError::Runtime(
            "Box build start command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn validate_inspect_command(
    build: &crate::modules::artifacts::domain::BuildRun,
    request: &NodeBoxBuildRequest,
    attempt: u32,
    command: &NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::BoxBuildInspect { request: admitted } = &command.payload else {
        return Err(FlowError::Runtime(
            "build inspection command is not a Box inspection".into(),
        ));
    };
    if command.id != inspect_command_id(build.id, attempt)
        || command.node_id
            != build
                .node_id
                .ok_or_else(|| FlowError::Runtime("scheduled build omitted its Box node".into()))?
        || command.aggregate_id != build.id.as_uuid()
        || command.correlation_id != build.operation_id.as_uuid()
        || admitted.as_ref() != request
    {
        return Err(FlowError::Runtime(
            "Box build inspection command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn result_deadline(
    runtime: &BuildFlowRuntime,
    command: &NodeCommand,
) -> a3s_flow::Result<DateTime<Utc>> {
    command
        .issued_at
        .checked_add_signed(runtime.config.execution_timeout)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("Box build execution deadline is invalid".into()))
}

fn start_command_id(build_id: BuildRunId) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(&build_id.as_uuid(), b"box-build-start"))
}

fn inspect_command_id(build_id: BuildRunId, attempt: u32) -> NodeCommandId {
    NodeCommandId::from_uuid(Uuid::new_v5(
        &build_id.as_uuid(),
        format!("box-build-inspect:{attempt}").as_bytes(),
    ))
}
