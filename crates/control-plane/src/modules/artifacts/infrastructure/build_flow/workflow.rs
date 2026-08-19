use super::types::{
    AttestStepInput, AttestStepOutput, BoxCleanupAction, BuildFlowInput, CleanupDispatchStepInput,
    CleanupDispatchStepOutput, CleanupObserveStepInput, CleanupObserveStepOutput,
    CompleteStepInput, CompleteStepOutput, DispatchStepInput, DispatchStepOutput, FailStepInput,
    FailStepOutput, ObserveStepInput, ObserveStepOutput, PreparePublicationStepInput,
    PreparePublicationStepOutput, PrepareStepOutput, PublishStepInput, PublishStepOutput,
    ScheduleStepInput, ScheduleStepOutput, ScheduledBuild, ValidateStepInput, ValidateStepOutput,
};
use super::{steps, BuildFlowConfig};
use crate::modules::artifacts::application::{BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION};
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
    if invocation.spec.version != BUILD_WORKFLOW_VERSION {
        return Err(FlowError::Runtime(format!(
            "Cloud has no build workflow runtime for {}@{}",
            invocation.spec.name, invocation.spec.version
        )));
    }
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
                    steps::BUILD_PREPARE_INPUT,
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
                                steps::BUILD_DISPATCH_BOX,
                                &DispatchStepInput { scheduled },
                            )
                        }
                    };
                if let Some(dispatched) = dispatched {
                    let box_output = match observe(config, &context, &input, dispatched)? {
                        Progress::Ready(output) => Some(output),
                        Progress::Failure(reason) => {
                            return failure_command(config, &context, &input, reason)
                        }
                        Progress::Cancellation => {
                            terminal_intent = Some(TerminalIntent::Cancellation);
                            None
                        }
                        Progress::Command(command) => return Ok(command),
                    };
                    if let Some(output_receipt) = box_output {
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
                                        steps::BUILD_VALIDATE_OUTPUT,
                                        &ValidateStepInput {
                                            flow: input.clone(),
                                            output: Box::new(output_receipt),
                                        },
                                    )
                                }
                            };
                        if let Some(output) = output {
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
                                        steps::BUILD_PREPARE_PUBLICATION,
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
                                        return failure_command(config, &context, &input, reason)
                                    }
                                    Some(PublishStepOutput::CancellationRequested { artifact }) => {
                                        terminal_intent = Some(TerminalIntent::Cancellation);
                                        artifact
                                    }
                                    None => {
                                        return stage_or_failure(
                                            config,
                                            &context,
                                            &input,
                                            PUBLISH_STEP_ID,
                                            steps::BUILD_PUBLISH_OUTPUT,
                                            &PublishStepInput {
                                                flow: input.clone(),
                                                output,
                                                target,
                                                deadline_at,
                                            },
                                        )
                                    }
                                };
                                if let Some(artifact) = published {
                                    match context
                                        .step_output_as::<AttestStepOutput>(ATTEST_STEP_ID)?
                                    {
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
                                                steps::BUILD_ATTEST_OUTPUT,
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
                                        "build attestation requires a published artifact".into(),
                                    ));
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
        steps::BUILD_COMPLETE,
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
            Some(ScheduleStepOutput::Ready { node_id, request }) => {
                return Ok(Progress::Ready(ScheduledBuild {
                    prepared,
                    node_id,
                    request: *request,
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
                    steps::BUILD_SCHEDULE_BOX,
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
) -> a3s_flow::Result<Progress<a3s_cloud_contracts::NodeBoxBuildOutput>> {
    let mut command_attempt = 1_u32;
    let mut poll = 1_u32;
    loop {
        let step_id = format!("observe-{command_attempt}-{poll}");
        match context.step_output_as::<ObserveStepOutput>(&step_id)? {
            Some(ObserveStepOutput::Succeeded { output, .. }) => {
                return Ok(Progress::Ready(*output))
            }
            Some(ObserveStepOutput::AwaitingCommand {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!("observe-wait-{command_attempt}-{poll}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                poll = next_attempt(poll)?;
            }
            Some(ObserveStepOutput::Running {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!("observe-wait-{command_attempt}-{poll}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                command_attempt = next_attempt(command_attempt)?;
                poll = 1;
            }
            Some(ObserveStepOutput::Failed { reason }) => return Ok(Progress::Failure(reason)),
            Some(ObserveStepOutput::CancellationRequested) => return Ok(Progress::Cancellation),
            None => {
                return stage_or_failure(
                    config,
                    context,
                    flow,
                    &step_id,
                    steps::BUILD_OBSERVE_BOX,
                    &ObserveStepInput {
                        dispatched: dispatched.clone(),
                        attempt: command_attempt,
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
    let mut action = BoxCleanupAction::Cancel;
    let mut attempt = 1_u32;
    let mut issued_at = None;
    let mut cleanup_deadline = None;
    loop {
        let dispatch_id = format!("cleanup-{}-dispatch-{attempt}", action.as_str());
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
                    steps::BUILD_CLEANUP_DISPATCH,
                    &CleanupDispatchStepInput {
                        flow: flow.clone(),
                        action,
                        attempt,
                        issued_at,
                        cleanup_deadline,
                    },
                )
                .map(CleanupProgress::Command)
            }
        };
        if dispatched.action != action || dispatched.attempt != attempt {
            return Err(FlowError::Runtime(
                "build cleanup dispatch changed its action or attempt".into(),
            ));
        }
        match observe_cleanup(config, context, flow, &dispatched)? {
            CleanupObserveProgress::Ready(cleaned_at) => {
                return Ok(CleanupProgress::Ready(cleaned_at))
            }
            CleanupObserveProgress::Advance {
                action: next_action,
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
                action = next_action;
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
        let observe_id = format!(
            "cleanup-{}-observe-{}-{poll}",
            dispatched.action.as_str(),
            dispatched.attempt
        );
        match context.step_output_as::<CleanupObserveStepOutput>(&observe_id)? {
            Some(CleanupObserveStepOutput::Ready { cleaned_at }) => {
                return Ok(CleanupObserveProgress::Ready(cleaned_at))
            }
            Some(CleanupObserveStepOutput::AwaitingCommand {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll(next_poll_at, deadline_at)?;
                let wait_id = format!(
                    "cleanup-{}-observe-wait-{}-{poll}",
                    dispatched.action.as_str(),
                    dispatched.attempt
                );
                if !context.wait_completed(&wait_id) {
                    return Ok(CleanupObserveProgress::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                poll = next_attempt(poll)?;
            }
            Some(CleanupObserveStepOutput::Advance {
                action,
                next_attempt_at,
                deadline_at,
                ..
            }) => {
                return Ok(CleanupObserveProgress::Advance {
                    action,
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
                    steps::BUILD_CLEANUP_OBSERVE,
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
        steps::BUILD_FAIL,
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

// These short-lived control results return RuntimeCommand immediately; boxing
// it would add a heap allocation to every dispatch, wait, or cleanup transition.
#[allow(clippy::large_enum_variant)]
enum Progress<T> {
    Ready(T),
    Failure(String),
    Cancellation,
    Command(RuntimeCommand),
}

#[allow(clippy::large_enum_variant)]
enum CleanupProgress {
    Ready(chrono::DateTime<chrono::Utc>),
    Command(RuntimeCommand),
}

#[allow(clippy::large_enum_variant)]
enum CleanupObserveProgress {
    Ready(chrono::DateTime<chrono::Utc>),
    Advance {
        action: BoxCleanupAction,
        next_attempt_at: chrono::DateTime<chrono::Utc>,
        deadline_at: chrono::DateTime<chrono::Utc>,
    },
    Command(RuntimeCommand),
}
