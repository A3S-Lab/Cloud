use super::types::{
    AttestStepInput, AttestStepOutput, BuildFlowInput, CleanupDispatchStepInput,
    CleanupDispatchStepOutput, CleanupObserveStepInput, CleanupObserveStepOutput,
    CompleteStepInput, CompleteStepOutput, DispatchStepInput, DispatchStepOutput, FailStepInput,
    FailStepOutput, ObserveStepInput, ObserveStepOutput, PreparePublicationStepInput,
    PreparePublicationStepOutput, PrepareStepOutput, PublishStepInput, PublishStepOutput,
    ScheduleStepInput, ScheduleStepOutput, ScheduledBuild, ValidateStepInput, ValidateStepOutput,
};
use super::BuildFlowConfig;
use crate::modules::artifacts::application::{
    BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION, LEGACY_BUILD_WORKFLOW_VERSION,
    PREVIOUS_BUILD_WORKFLOW_VERSION,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};

const PREPARE_STEP_ID: &str = "prepare";
const DISPATCH_STEP_ID: &str = "dispatch";
const VALIDATE_STEP_ID: &str = "validate";
const PREPARE_PUBLICATION_STEP_ID: &str = "publication-target";
const PUBLISH_STEP_ID: &str = "publish";
const ATTEST_STEP_ID: &str = "attest";
const FAIL_STEP_ID: &str = "fail";
const COMPLETE_STEP_ID: &str = "complete";

pub(super) fn replay(
    config: &BuildFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    if invocation.spec.name != BUILD_WORKFLOW_NAME {
        return Err(FlowError::Runtime(format!(
            "Cloud has no build workflow runtime for {}@{}",
            invocation.spec.name, invocation.spec.version
        )));
    }
    let (requires_publication, requires_evidence) = match invocation.spec.version.as_str() {
        BUILD_WORKFLOW_VERSION => (true, true),
        PREVIOUS_BUILD_WORKFLOW_VERSION => (true, false),
        LEGACY_BUILD_WORKFLOW_VERSION => (false, false),
        _ => {
            return Err(FlowError::Runtime(format!(
                "Cloud has no build workflow runtime for {}@{}",
                invocation.spec.name, invocation.spec.version
            )))
        }
    };
    let context = invocation.context();
    let input = context.input_as::<BuildFlowInput>()?;
    if let Some(completed) = context.step_output_as::<CompleteStepOutput>(COMPLETE_STEP_ID)? {
        return terminal_command(&context, completed);
    }

    let mut terminal_intent = context
        .step_output_as::<FailStepOutput>(FAIL_STEP_ID)?
        .map(|_| TerminalIntent::Failure);

    if terminal_intent.is_none() {
        let prepared = match context.step_output_as::<PrepareStepOutput>(PREPARE_STEP_ID)? {
            Some(PrepareStepOutput::Ready { prepared }) => Some(*prepared),
            Some(PrepareStepOutput::Failed { reason }) => {
                return failure_command(config, &context, &input, reason)
            }
            Some(PrepareStepOutput::Rejected { reason }) => return Ok(context.fail(reason)),
            Some(PrepareStepOutput::CancellationRequested) => {
                terminal_intent = Some(TerminalIntent::Cancellation);
                None
            }
            None => {
                return stage_or_failure(
                    config,
                    &context,
                    &input,
                    PREPARE_STEP_ID,
                    "build_prepare_input",
                    &input,
                )
            }
        };

        if let Some(prepared) = prepared {
            let scheduled = match schedule(config, &context, &input, prepared)? {
                Progress::Ready(scheduled) => Some(scheduled),
                Progress::Failure(reason) => {
                    return failure_command(config, &context, &input, reason)
                }
                Progress::Cancellation => {
                    terminal_intent = Some(TerminalIntent::Cancellation);
                    None
                }
                Progress::Command(command) => return Ok(command),
            };
            if let Some(scheduled) = scheduled {
                let dispatched =
                    match context.step_output_as::<DispatchStepOutput>(DISPATCH_STEP_ID)? {
                        Some(DispatchStepOutput::Ready { dispatched }) => Some(*dispatched),
                        Some(DispatchStepOutput::Failed { reason }) => {
                            return failure_command(config, &context, &input, reason)
                        }
                        Some(DispatchStepOutput::CancellationRequested) => {
                            terminal_intent = Some(TerminalIntent::Cancellation);
                            None
                        }
                        None => {
                            return stage_or_failure(
                                config,
                                &context,
                                &input,
                                DISPATCH_STEP_ID,
                                "build_dispatch_runtime",
                                &DispatchStepInput { scheduled },
                            )
                        }
                    };
                if let Some(dispatched) = dispatched {
                    let artifact = match observe(config, &context, &input, dispatched)? {
                        Progress::Ready(artifact) => Some(artifact),
                        Progress::Failure(reason) => {
                            return failure_command(config, &context, &input, reason)
                        }
                        Progress::Cancellation => {
                            terminal_intent = Some(TerminalIntent::Cancellation);
                            None
                        }
                        Progress::Command(command) => return Ok(command),
                    };
                    if let Some(artifact) = artifact {
                        let output =
                            match context.step_output_as::<ValidateStepOutput>(VALIDATE_STEP_ID)? {
                                Some(ValidateStepOutput::Ready { output }) => Some(output),
                                Some(ValidateStepOutput::Failed { reason }) => {
                                    return failure_command(config, &context, &input, reason)
                                }
                                Some(ValidateStepOutput::CancellationRequested) => {
                                    terminal_intent = Some(TerminalIntent::Cancellation);
                                    None
                                }
                                None => {
                                    return stage_or_failure(
                                        config,
                                        &context,
                                        &input,
                                        VALIDATE_STEP_ID,
                                        "build_validate_output",
                                        &ValidateStepInput {
                                            flow: input.clone(),
                                            artifact,
                                        },
                                    )
                                }
                            };
                        if let Some(output) = output {
                            if !requires_publication {
                                terminal_intent = Some(TerminalIntent::Success);
                            } else {
                                let publication = match context
                                    .step_output_as::<PreparePublicationStepOutput>(
                                        PREPARE_PUBLICATION_STEP_ID,
                                    )? {
                                    Some(PreparePublicationStepOutput::Ready {
                                        target,
                                        deadline_at,
                                    }) => Some((target, deadline_at)),
                                    Some(PreparePublicationStepOutput::Failed { reason }) => {
                                        return failure_command(config, &context, &input, reason)
                                    }
                                    Some(PreparePublicationStepOutput::CancellationRequested) => {
                                        terminal_intent = Some(TerminalIntent::Cancellation);
                                        None
                                    }
                                    None => {
                                        return stage_or_failure(
                                            config,
                                            &context,
                                            &input,
                                            PREPARE_PUBLICATION_STEP_ID,
                                            "build_prepare_publication",
                                            &PreparePublicationStepInput {
                                                flow: input.clone(),
                                                output: output.clone(),
                                            },
                                        )
                                    }
                                };
                                if let Some((target, deadline_at)) = publication {
                                    let published = match context
                                        .step_output_as::<PublishStepOutput>(PUBLISH_STEP_ID)?
                                    {
                                        Some(PublishStepOutput::Ready { artifact }) => {
                                            terminal_intent = Some(TerminalIntent::Success);
                                            Some(artifact)
                                        }
                                        Some(PublishStepOutput::Failed { reason }) => {
                                            return failure_command(
                                                config, &context, &input, reason,
                                            )
                                        }
                                        Some(PublishStepOutput::CancellationRequested {
                                            artifact,
                                        }) => {
                                            terminal_intent = Some(TerminalIntent::Cancellation);
                                            artifact
                                        }
                                        None => {
                                            return stage_or_failure(
                                                config,
                                                &context,
                                                &input,
                                                PUBLISH_STEP_ID,
                                                "build_publish_output",
                                                &PublishStepInput {
                                                    flow: input.clone(),
                                                    output,
                                                    target,
                                                    deadline_at,
                                                },
                                            )
                                        }
                                    };
                                    if requires_evidence {
                                        if let Some(artifact) = published {
                                            match context.step_output_as::<AttestStepOutput>(
                                                ATTEST_STEP_ID,
                                            )? {
                                                Some(AttestStepOutput::Ready { .. }) => {}
                                                Some(AttestStepOutput::Failed { reason }) => {
                                                    return failure_command(
                                                        config, &context, &input, reason,
                                                    )
                                                }
                                                None => {
                                                    return stage_or_failure(
                                                        config,
                                                        &context,
                                                        &input,
                                                        ATTEST_STEP_ID,
                                                        "build_attest_output",
                                                        &AttestStepInput {
                                                            flow: input.clone(),
                                                            artifact,
                                                        },
                                                    )
                                                }
                                            }
                                        } else if !matches!(
                                            terminal_intent.as_ref(),
                                            Some(TerminalIntent::Cancellation)
                                        ) {
                                            return Err(FlowError::Runtime(
                                                "build attestation requires a published artifact"
                                                    .into(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if terminal_intent.is_none() {
        return Err(FlowError::Runtime(
            "build workflow reached cleanup without a terminal intent".into(),
        ));
    }
    let cleaned_at = match cleanup(config, &context, &input)? {
        CleanupProgress::Ready(cleaned_at) => cleaned_at,
        CleanupProgress::Command(command) => return Ok(command),
    };
    stage_or_failure(
        config,
        &context,
        &input,
        COMPLETE_STEP_ID,
        "build_complete",
        &CompleteStepInput {
            flow: input.clone(),
            cleaned_at,
        },
    )
}

fn terminal_command(
    context: &WorkflowContext<'_>,
    completed: CompleteStepOutput,
) -> a3s_flow::Result<RuntimeCommand> {
    match completed.status {
        crate::modules::artifacts::domain::BuildRunStatus::Succeeded
            if completed.failure.is_none() =>
        {
            Ok(context.complete(serde_json::to_value(completed)?))
        }
        crate::modules::artifacts::domain::BuildRunStatus::Cancelled
            if completed.failure.is_none() =>
        {
            Ok(context.complete(serde_json::to_value(completed)?))
        }
        crate::modules::artifacts::domain::BuildRunStatus::Failed
        | crate::modules::artifacts::domain::BuildRunStatus::Cancelled => Ok(context.fail(
            completed
                .failure
                .unwrap_or_else(|| "build failed without a persisted reason".into()),
        )),
        status => Err(FlowError::Runtime(format!(
            "completed build workflow retained non-terminal status {}",
            status.as_str()
        ))),
    }
}

fn schedule(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
    prepared: super::types::PreparedBuild,
) -> a3s_flow::Result<Progress<ScheduledBuild>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("schedule-{attempt}");
        match context.step_output_as::<ScheduleStepOutput>(&step_id)? {
            Some(ScheduleStepOutput::Ready { node_id, spec }) => {
                return Ok(Progress::Ready(ScheduledBuild {
                    prepared,
                    node_id,
                    spec: *spec,
                }))
            }
            Some(ScheduleStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!("schedule-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt)?;
            }
            Some(ScheduleStepOutput::Failed { reason }) => return Ok(Progress::Failure(reason)),
            Some(ScheduleStepOutput::CancellationRequested) => return Ok(Progress::Cancellation),
            None => {
                return stage_or_failure(
                    config,
                    context,
                    flow,
                    &step_id,
                    "build_schedule_runtime",
                    &ScheduleStepInput {
                        prepared: prepared.clone(),
                    },
                )
                .map(Progress::Command)
            }
        }
    }
}

fn observe(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
    dispatched: super::types::DispatchedBuild,
) -> a3s_flow::Result<Progress<crate::modules::artifacts::domain::BuildArtifact>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("observe-{attempt}");
        match context.step_output_as::<ObserveStepOutput>(&step_id)? {
            Some(ObserveStepOutput::Succeeded { artifact, .. }) => {
                return Ok(Progress::Ready(artifact))
            }
            Some(ObserveStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!("observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt)?;
            }
            Some(ObserveStepOutput::Failed { reason }) => return Ok(Progress::Failure(reason)),
            Some(ObserveStepOutput::CancellationRequested) => return Ok(Progress::Cancellation),
            None => {
                return stage_or_failure(
                    config,
                    context,
                    flow,
                    &step_id,
                    "build_observe_runtime",
                    &ObserveStepInput {
                        dispatched: dispatched.clone(),
                    },
                )
                .map(Progress::Command)
            }
        }
    }
}

fn cleanup(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
) -> a3s_flow::Result<CleanupProgress> {
    let mut attempt = 1_u32;
    let mut issued_at = None;
    let mut cleanup_deadline = None;
    loop {
        let dispatch_id = format!("cleanup-dispatch-{attempt}");
        let dispatched = match context.step_output_as::<CleanupDispatchStepOutput>(&dispatch_id)? {
            Some(CleanupDispatchStepOutput::NotRequired { cleaned_at }) => {
                return Ok(CleanupProgress::Ready(cleaned_at))
            }
            Some(CleanupDispatchStepOutput::Ready { dispatched }) => dispatched,
            Some(CleanupDispatchStepOutput::Retry {
                next_attempt_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_attempt_at, deadline_at)?;
                let wait_id = format!("cleanup-dispatch-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(CleanupProgress::Command(
                        context.wait_until(wait_id, next_attempt_at),
                    ));
                }
                issued_at = Some(next_attempt_at);
                cleanup_deadline = Some(deadline_at);
                attempt = next_attempt(attempt)?;
                continue;
            }
            None => {
                return stage_or_failure(
                    config,
                    context,
                    flow,
                    &dispatch_id,
                    "build_cleanup_dispatch",
                    &CleanupDispatchStepInput {
                        flow: flow.clone(),
                        attempt,
                        issued_at,
                        cleanup_deadline,
                    },
                )
                .map(CleanupProgress::Command)
            }
        };
        if dispatched.attempt != attempt {
            return Err(FlowError::Runtime(
                "build cleanup dispatch changed its attempt".into(),
            ));
        }
        match observe_cleanup(config, context, flow, &dispatched)? {
            CleanupObserveProgress::Ready(cleaned_at) => {
                return Ok(CleanupProgress::Ready(cleaned_at))
            }
            CleanupObserveProgress::Retry {
                next_attempt_at,
                deadline_at,
            } => {
                validate_poll(next_attempt_at, deadline_at)?;
                let wait_id = format!("cleanup-retry-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(CleanupProgress::Command(
                        context.wait_until(wait_id, next_attempt_at),
                    ));
                }
                issued_at = Some(next_attempt_at);
                cleanup_deadline = Some(deadline_at);
                attempt = next_attempt(attempt)?;
            }
            CleanupObserveProgress::Command(command) => {
                return Ok(CleanupProgress::Command(command))
            }
        }
    }
}

fn observe_cleanup(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
    dispatched: &super::types::DispatchedCleanup,
) -> a3s_flow::Result<CleanupObserveProgress> {
    let mut poll = 1_u32;
    loop {
        let observe_id = format!("cleanup-observe-{}-{poll}", dispatched.attempt);
        match context.step_output_as::<CleanupObserveStepOutput>(&observe_id)? {
            Some(CleanupObserveStepOutput::Ready { cleaned_at }) => {
                return Ok(CleanupObserveProgress::Ready(cleaned_at))
            }
            Some(CleanupObserveStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!("cleanup-observe-wait-{}-{poll}", dispatched.attempt);
                if !context.wait_completed(&wait_id) {
                    return Ok(CleanupObserveProgress::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                poll = next_attempt(poll)?;
            }
            Some(CleanupObserveStepOutput::Retry {
                next_attempt_at,
                deadline_at,
                ..
            }) => {
                return Ok(CleanupObserveProgress::Retry {
                    next_attempt_at,
                    deadline_at,
                })
            }
            None => {
                return stage_or_failure(
                    config,
                    context,
                    flow,
                    &observe_id,
                    "build_cleanup_observe",
                    &CleanupObserveStepInput {
                        flow: flow.clone(),
                        dispatched: dispatched.clone(),
                    },
                )
                .map(CleanupObserveProgress::Command)
            }
        }
    }
}

fn stage_or_failure<T: serde::Serialize>(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return failure_command(
            config,
            context,
            flow,
            format!("build stage {step_name} failed: {error}"),
        );
    }
    Ok(context.schedule_step_with_retry(
        step_id,
        step_name,
        serde_json::to_value(input)?,
        config.retry_policy(),
    ))
}

fn failure_command(
    config: &BuildFlowConfig,
    context: &WorkflowContext<'_>,
    flow: &BuildFlowInput,
    reason: String,
) -> a3s_flow::Result<RuntimeCommand> {
    if context.step_failed(FAIL_STEP_ID).is_some() {
        return Err(FlowError::Runtime(
            "build failure could not be persisted".into(),
        ));
    }
    Ok(context.schedule_step_with_retry(
        FAIL_STEP_ID,
        "build_fail",
        serde_json::to_value(FailStepInput {
            flow: flow.clone(),
            reason,
        })?,
        config.retry_policy(),
    ))
}

fn validate_poll(
    next_at: chrono::DateTime<chrono::Utc>,
    deadline_at: chrono::DateTime<chrono::Utc>,
) -> a3s_flow::Result<()> {
    if next_at > deadline_at {
        return Err(FlowError::Runtime(
            "build poll exceeds its durable deadline".into(),
        ));
    }
    Ok(())
}

fn next_attempt(attempt: u32) -> a3s_flow::Result<u32> {
    attempt
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime("build attempt overflowed".into()))
}

enum TerminalIntent {
    Success,
    Failure,
    Cancellation,
}

enum Progress<T> {
    Ready(T),
    Failure(String),
    Cancellation,
    Command(RuntimeCommand),
}

enum CleanupProgress {
    Ready(chrono::DateTime<chrono::Utc>),
    Command(RuntimeCommand),
}

enum CleanupObserveProgress {
    Ready(chrono::DateTime<chrono::Utc>),
    Retry {
        next_attempt_at: chrono::DateTime<chrono::Utc>,
        deadline_at: chrono::DateTime<chrono::Utc>,
    },
    Command(RuntimeCommand),
}
