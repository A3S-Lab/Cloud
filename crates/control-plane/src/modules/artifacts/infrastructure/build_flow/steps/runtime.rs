use super::super::task_spec::OUTPUT_NAME;
use super::super::types::{
    DispatchStepInput, DispatchStepOutput, DispatchedBuild, ObserveStepInput, ObserveStepOutput,
    ScheduleStepInput, ScheduleStepOutput,
};
use super::super::{flow_error, BuildFlowRuntime};
use super::common::{
    bounded_reason, load_build, load_revision, next_poll, project_spec, timestamp_millis,
};
use crate::modules::artifacts::domain::{BuildArtifact, BuildRunStatus};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::shared_kernel::domain::NodeCommandId;
use a3s_cloud_contracts::{
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_flow::FlowError;
use a3s_runtime::contract::{
    RuntimeApplyRequest, RuntimeCapabilities, RuntimeObservation, RuntimeUnitState,
};
use chrono::{DateTime, Utc};

pub(super) async fn schedule(
    runtime: &BuildFlowRuntime,
    run_id: &str,
    input: ScheduleStepInput,
) -> a3s_flow::Result<ScheduleStepOutput> {
    let mut build = load_build(
        runtime,
        run_id,
        &super::super::types::BuildFlowInput {
            organization_id: input.prepared.organization_id,
            build_run_id: input.prepared.build_run_id,
        },
    )
    .await?;
    validate_prepared(&build, &input)?;
    if build.cancellation_requested_at.is_some() {
        return Ok(ScheduleStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(ScheduleStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    let revision = load_revision(runtime, &build).await?;
    let spec = project_spec(runtime, &build, &revision)?;
    let spec_digest = spec
        .digest()
        .map_err(|error| flow_error("could not digest build Runtime Task", error))?;
    if let Some(node_id) = build.node_id {
        if build.runtime_spec_digest.as_deref() != Some(spec_digest.as_str()) {
            return Err(FlowError::Runtime(
                "scheduled build Runtime specification changed during replay".into(),
            ));
        }
        return Ok(ScheduleStepOutput::Ready {
            node_id,
            spec: Box::new(spec),
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
        .map_err(|error| flow_error("could not list build Runtime nodes", error))?;
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
                tracing::warn!(node_id = %node.id, error = %error, "ignoring invalid Runtime capabilities during build scheduling");
                continue;
            }
        };
        if !capabilities
            .missing_for(&spec)
            .map_err(|error| flow_error("could not match build Runtime capabilities", error))?
            .is_empty()
        {
            continue;
        }
        let expected = build.aggregate_version;
        build
            .schedule(node.id, spec_digest.clone(), now)
            .map_err(|error| flow_error("could not schedule build Runtime Task", error))?;
        let scheduled = runtime
            .builds
            .save(build, expected)
            .await
            .map_err(|error| flow_error("could not persist build Runtime schedule", error))?;
        return Ok(ScheduleStepOutput::Ready {
            node_id: scheduled.node_id.ok_or_else(|| {
                FlowError::Runtime("scheduled build omitted its Runtime node".into())
            })?,
            spec: Box::new(spec),
        });
    }
    if now >= input.prepared.convergence_deadline {
        return Ok(ScheduleStepOutput::Failed {
            reason: "no ready node satisfied the isolated build Runtime specification before its deadline".into(),
        });
    }
    Ok(ScheduleStepOutput::Pending {
        reason:
            "waiting for a node with Task, Artifact, output, volume, and network-none capabilities"
                .into(),
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
            .map_err(|error| flow_error("could not reload build Runtime command", error))?
            .ok_or_else(|| {
                FlowError::Runtime("dispatched build Runtime command is missing".into())
            })?;
        validate_apply_command(&build, &input.scheduled.spec, &command)?;
        return Ok(DispatchStepOutput::Ready {
            dispatched: Box::new(DispatchedBuild {
                scheduled: input.scheduled,
                command_id,
                result_deadline: apply_result_deadline(&command)?,
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
        .ok_or_else(|| FlowError::Runtime("build command deadline overflowed".into()))?;
    let runtime_deadline = issued_at
        .checked_add_signed(runtime.config.execution_timeout)
        .ok_or_else(|| FlowError::Runtime("build Runtime deadline overflowed".into()))?;
    let result_deadline = not_after.min(runtime_deadline);
    if Utc::now() >= result_deadline {
        return Ok(DispatchStepOutput::Failed {
            reason: "build Runtime command expired before dispatch".into(),
        });
    }
    let command_id = NodeCommandId::from_uuid(build.id.as_uuid());
    let payload = NodeCommandPayload::RuntimeApply {
        request: Box::new(RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("build:{}:apply", build.id),
            deadline_at_ms: Some(timestamp_millis(runtime_deadline)?),
            spec: input.scheduled.spec.clone(),
        }),
    };
    let command = runtime
        .node_control
        .enqueue_command(NodeCommandDraft {
            proposed_command_id: command_id,
            node_id: input.scheduled.node_id,
            aggregate_id: build.id.as_uuid(),
            payload,
            issued_at,
            not_after,
            correlation_id: build.operation_id.as_uuid(),
        })
        .await
        .map_err(|error| flow_error("could not enqueue build Runtime command", error))?
        .value;
    validate_apply_command(&build, &input.scheduled.spec, &command)?;
    let expected = build.aggregate_version;
    build
        .dispatch(command.id, Utc::now().max(build.updated_at))
        .map_err(|error| flow_error("could not mark build Runtime dispatch", error))?;
    let dispatched = runtime
        .builds
        .save(build, expected)
        .await
        .map_err(|error| flow_error("could not persist build Runtime dispatch", error))?;
    Ok(DispatchStepOutput::Ready {
        dispatched: Box::new(DispatchedBuild {
            scheduled: input.scheduled,
            command_id: dispatched.command_id.ok_or_else(|| {
                FlowError::Runtime("dispatched build omitted its Runtime command".into())
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
    let flow = super::super::types::BuildFlowInput {
        organization_id: input.dispatched.scheduled.prepared.organization_id,
        build_run_id: input.dispatched.scheduled.prepared.build_run_id,
    };
    let build = load_build(runtime, run_id, &flow).await?;
    validate_dispatched(&build, &input)?;
    if build.cancellation_requested_at.is_some() {
        return Ok(ObserveStepOutput::CancellationRequested);
    }
    if let Some(reason) = &build.failure {
        return Ok(ObserveStepOutput::Failed {
            reason: reason.clone(),
        });
    }
    if let Some(artifact) = &build.runtime_output_artifact {
        return Ok(ObserveStepOutput::Succeeded {
            artifact: artifact.clone(),
            completed_at: build.updated_at,
        });
    }
    if let Some(record) = runtime
        .node_control
        .latest_runtime_observation(
            input.dispatched.scheduled.node_id,
            &input.dispatched.scheduled.spec.unit_id,
            input.dispatched.scheduled.spec.generation,
        )
        .await
        .map_err(|error| flow_error("could not load build Runtime observation", error))?
    {
        if record.command_id != Some(input.dispatched.command_id) {
            return Err(FlowError::Runtime(
                "build Runtime observation belongs to another command".into(),
            ));
        }
        if let Some(output) = consume_observation(
            runtime,
            build.clone(),
            &input,
            record.observation,
            record.received_at,
        )
        .await?
        {
            return Ok(output);
        }
    }
    if let Some(acknowledgement) = runtime
        .node_control
        .command_acknowledgement(
            input.dispatched.scheduled.node_id,
            input.dispatched.command_id,
        )
        .await
        .map_err(|error| flow_error("could not load build Runtime command result", error))?
    {
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::RuntimeApplied { observation } => {
                    if let Some(output) = consume_observation(
                        runtime,
                        build,
                        &input,
                        *observation,
                        acknowledgement.completed_at,
                    )
                    .await?
                    {
                        return Ok(output);
                    }
                }
                _ => {
                    return Err(FlowError::Runtime(
                        "build apply command returned another result kind".into(),
                    ))
                }
            },
            NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
                return Ok(ObserveStepOutput::Failed {
                    reason: bounded_reason(format!("{}: {}", failure.code, failure.message)),
                })
            }
        }
    }
    let now = Utc::now();
    let deadline = input
        .dispatched
        .result_deadline
        .min(input.dispatched.scheduled.prepared.convergence_deadline);
    if now >= deadline {
        return Ok(ObserveStepOutput::Failed {
            reason: "build Runtime Task did not finish before its deadline".into(),
        });
    }
    Ok(ObserveStepOutput::Pending {
        reason: "waiting for terminal build Runtime output".into(),
        next_poll_at: next_poll(now, runtime.config.observation_poll, deadline)?,
        deadline_at: deadline,
    })
}

async fn consume_observation(
    runtime: &BuildFlowRuntime,
    mut build: crate::modules::artifacts::domain::BuildRun,
    input: &ObserveStepInput,
    observation: RuntimeObservation,
    completed_at: DateTime<Utc>,
) -> a3s_flow::Result<Option<ObserveStepOutput>> {
    observation
        .validate_against(&input.dispatched.scheduled.spec)
        .map_err(|error| flow_error("build Runtime observation is inconsistent", error))?;
    match observation.state {
        RuntimeUnitState::Succeeded => {
            let [output] = observation.outputs.as_slice() else {
                return Ok(Some(ObserveStepOutput::Failed {
                    reason:
                        "successful build Runtime Task did not produce exactly one output Artifact"
                            .into(),
                }));
            };
            if output.name != OUTPUT_NAME
                || output.artifact.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
                || output.size_bytes == 0
                || output.size_bytes > runtime.config.output_max_bytes
            {
                return Ok(Some(ObserveStepOutput::Failed {
                    reason: "build Runtime output changed its declared identity or bound".into(),
                }));
            }
            let artifact = BuildArtifact::new(
                output.artifact.uri.clone(),
                output.artifact.digest.clone(),
                output.artifact.media_type.clone(),
                output.size_bytes,
            )
            .map_err(|error| flow_error("build Runtime output is invalid", error))?;
            let expected = build.aggregate_version;
            build
                .begin_validation(artifact.clone(), completed_at.max(build.updated_at))
                .map_err(|error| flow_error("could not begin build output validation", error))?;
            runtime
                .builds
                .save(build, expected)
                .await
                .map_err(|error| flow_error("could not persist build Runtime output", error))?;
            Ok(Some(ObserveStepOutput::Succeeded {
                artifact,
                completed_at,
            }))
        }
        RuntimeUnitState::Failed => Ok(Some(ObserveStepOutput::Failed {
            reason: bounded_reason(
                observation
                    .failure
                    .map(|failure| format!("{}: {}", failure.code, failure.message))
                    .unwrap_or_else(|| "build Runtime Task failed".into()),
            ),
        })),
        state if state.is_terminal() => Ok(Some(ObserveStepOutput::Failed {
            reason: format!("build Runtime Task terminated in unexpected state {state:?}"),
        })),
        _ => Ok(None),
    }
}

fn validate_prepared(
    build: &crate::modules::artifacts::domain::BuildRun,
    input: &ScheduleStepInput,
) -> a3s_flow::Result<()> {
    if build.id != input.prepared.build_run_id
        || build.organization_id != input.prepared.organization_id
        || build.source_revision_id != input.prepared.source_revision_id
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
        .spec
        .digest()
        .map_err(|error| flow_error("scheduled build Runtime Task is invalid", error))?;
    if build.node_id != Some(input.scheduled.node_id)
        || build.runtime_spec_digest.as_deref() != Some(digest.as_str())
    {
        return Err(FlowError::Runtime(
            "scheduled build step input changed Runtime identity".into(),
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
            "dispatched build step input changed Runtime command identity".into(),
        ));
    }
    Ok(())
}

fn validate_apply_command(
    build: &crate::modules::artifacts::domain::BuildRun,
    spec: &a3s_runtime::contract::RuntimeUnitSpec,
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<()> {
    let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "build command is not a Runtime apply".into(),
        ));
    };
    if command.id != NodeCommandId::from_uuid(build.id.as_uuid())
        || command.node_id
            != build.node_id.ok_or_else(|| {
                FlowError::Runtime("scheduled build omitted its Runtime node".into())
            })?
        || command.aggregate_id != build.id.as_uuid()
        || command.correlation_id != build.operation_id.as_uuid()
        || request.request_id != format!("build:{}:apply", build.id)
        || request.spec != *spec
    {
        return Err(FlowError::Runtime(
            "build Runtime command changed its durable identity".into(),
        ));
    }
    Ok(())
}

fn apply_result_deadline(
    command: &crate::modules::fleet::domain::entities::NodeCommand,
) -> a3s_flow::Result<DateTime<Utc>> {
    let NodeCommandPayload::RuntimeApply { request } = &command.payload else {
        return Err(FlowError::Runtime(
            "build command is not a Runtime apply".into(),
        ));
    };
    let millis = request
        .deadline_at_ms
        .ok_or_else(|| FlowError::Runtime("build Runtime apply omitted its deadline".into()))?;
    let millis = i64::try_from(millis)
        .map_err(|_| FlowError::Runtime("build Runtime deadline is invalid".into()))?;
    DateTime::from_timestamp_millis(millis)
        .map(|deadline| deadline.min(command.not_after))
        .ok_or_else(|| FlowError::Runtime("build Runtime deadline is invalid".into()))
}
