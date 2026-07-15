use super::types::{
    ActivateStepInput, ActivateStepOutput, CleanupDispatchStepInput, CleanupDispatchStepOutput,
    CleanupObserveStepInput, CleanupObserveStepOutput, CompleteCancellationStepInput,
    CompleteCancellationStepOutput, DeploymentFlowInput, DispatchStepInput, DispatchStepOutput,
    DispatchedCleanup, DispatchedRuntime, FailStepInput, FailStepOutput, ObserveStepInput,
    ObserveStepOutput, ResolveStepOutput, ResolveStepResult, ScheduleStepInput, ScheduleStepOutput,
    VerifyStepInput, VerifyStepOutput,
};
use super::DeploymentFlowConfig;
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};

const RESOLVE_STEP_ID: &str = "resolve";
const DISPATCH_STEP_ID: &str = "dispatch";
const VERIFY_STEP_ID: &str = "verify";
const ACTIVATE_STEP_ID: &str = "activate";
const FAIL_STEP_ID: &str = "fail";
const COMPLETE_CANCELLATION_STEP_ID: &str = "complete-cancellation";

pub(super) fn replay(
    config: &DeploymentFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    let context = invocation.context();
    let input = context.input_as::<DeploymentFlowInput>()?;

    if let Some(failed) = context.step_output_as::<FailStepOutput>(FAIL_STEP_ID)? {
        return Ok(context.fail(failed.reason));
    }

    let resolved = match context.step_output_as::<ResolveStepResult>(RESOLVE_STEP_ID)? {
        Some(ResolveStepResult::Resolved(output)) => *output,
        Some(ResolveStepResult::CancellationRequested(output)) => {
            return complete_cancellation_command(config, &context, &input, output.cleaned_at)
        }
        None => {
            return stage_or_failure(
                config,
                &context,
                &input,
                RESOLVE_STEP_ID,
                "resolve_deployment",
                &input,
            )
        }
    };

    let node_id = match schedule_node(config, &context, &input, &resolved)? {
        Progress::Ready(node_id) => node_id,
        Progress::Cancellation => return cancel_deployment(config, &context, &input, &resolved),
        Progress::Command(command) => return Ok(command),
    };
    let dispatched = match context.step_output_as::<DispatchStepOutput>(DISPATCH_STEP_ID)? {
        Some(DispatchStepOutput::Ready { dispatched }) => dispatched,
        Some(DispatchStepOutput::Failed { reason }) => {
            return failure_command(config, &context, &input, reason)
        }
        Some(DispatchStepOutput::CancellationRequested) => {
            return cancel_deployment(config, &context, &input, &resolved)
        }
        None => {
            return stage_or_failure(
                config,
                &context,
                &input,
                DISPATCH_STEP_ID,
                "dispatch_runtime_apply",
                &DispatchStepInput {
                    resolved: resolved.clone(),
                    node_id,
                },
            )
        }
    };
    if dispatched.node_id != node_id {
        return Err(FlowError::Runtime(
            "deployment dispatch changed its scheduled node".into(),
        ));
    }

    let observation = match observe_runtime(config, &context, &input, &resolved, &dispatched)? {
        Progress::Ready(observation) => observation,
        Progress::Cancellation => return cancel_deployment(config, &context, &input, &resolved),
        Progress::Command(command) => return Ok(command),
    };
    let verification = match context.step_output_as::<VerifyStepOutput>(VERIFY_STEP_ID)? {
        Some(VerifyStepOutput::CancellationRequested) => {
            return cancel_deployment(config, &context, &input, &resolved)
        }
        Some(output @ VerifyStepOutput::Verified { .. }) => output,
        None => {
            return stage_or_failure(
                config,
                &context,
                &input,
                VERIFY_STEP_ID,
                "verify_runtime_health",
                &VerifyStepInput {
                    resolved: resolved.clone(),
                    observation,
                },
            )
        }
    };
    let activation = match context.step_output_as::<ActivateStepOutput>(ACTIVATE_STEP_ID)? {
        Some(ActivateStepOutput::CancellationRequested) => {
            return cancel_deployment(config, &context, &input, &resolved)
        }
        Some(output @ ActivateStepOutput::Active { .. }) => output,
        None => {
            return stage_or_failure(
                config,
                &context,
                &input,
                ACTIVATE_STEP_ID,
                "activate_deployment",
                &ActivateStepInput {
                    resolved,
                    verification,
                },
            )
        }
    };

    Ok(context.complete(serde_json::to_value(activation)?))
}

fn schedule_node(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    resolved: &ResolveStepOutput,
) -> a3s_flow::Result<Progress<crate::modules::shared_kernel::domain::NodeId>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("schedule-{attempt}");
        match context.step_output_as::<ScheduleStepOutput>(&step_id)? {
            Some(ScheduleStepOutput::Ready { node_id }) => return Ok(Progress::Ready(node_id)),
            Some(ScheduleStepOutput::Failed { reason }) => {
                return failure_command(config, context, flow_input, reason).map(Progress::Command)
            }
            Some(ScheduleStepOutput::CancellationRequested) => return Ok(Progress::Cancellation),
            Some(ScheduleStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                if next_poll_at > deadline_at {
                    return Err(FlowError::Runtime(
                        "scheduler poll exceeds the convergence deadline".into(),
                    ));
                }
                let wait_id = format!("schedule-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = attempt
                    .checked_add(1)
                    .ok_or_else(|| FlowError::Runtime("scheduler attempt overflowed".into()))?;
            }
            None => {
                let command = stage_or_failure(
                    config,
                    context,
                    flow_input,
                    &step_id,
                    "schedule_deployment",
                    &ScheduleStepInput {
                        resolved: resolved.clone(),
                    },
                )?;
                return Ok(Progress::Command(command));
            }
        }
    }
}

fn observe_runtime(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    resolved: &ResolveStepOutput,
    dispatched: &DispatchedRuntime,
) -> a3s_flow::Result<Progress<ObserveStepOutput>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("observe-{attempt}");
        match context.step_output_as::<ObserveStepOutput>(&step_id)? {
            Some(ready @ ObserveStepOutput::Ready { .. }) => return Ok(Progress::Ready(ready)),
            Some(ObserveStepOutput::Failed { reason }) => {
                return failure_command(config, context, flow_input, reason).map(Progress::Command)
            }
            Some(ObserveStepOutput::CancellationRequested) => return Ok(Progress::Cancellation),
            Some(ObserveStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                if next_poll_at > deadline_at {
                    return Err(FlowError::Runtime(
                        "observation poll exceeds the convergence deadline".into(),
                    ));
                }
                let wait_id = format!("observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = attempt
                    .checked_add(1)
                    .ok_or_else(|| FlowError::Runtime("observation attempt overflowed".into()))?;
            }
            None => {
                let command = stage_or_failure(
                    config,
                    context,
                    flow_input,
                    &step_id,
                    "observe_runtime_apply",
                    &ObserveStepInput {
                        resolved: resolved.clone(),
                        dispatched: dispatched.clone(),
                    },
                )?;
                return Ok(Progress::Command(command));
            }
        }
    }
}

fn cancel_deployment(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    resolved: &ResolveStepOutput,
) -> a3s_flow::Result<RuntimeCommand> {
    let mut attempt = 1_u32;
    let mut issued_at = None;
    loop {
        let dispatch_step_id = format!("cleanup-dispatch-{attempt}");
        let dispatched =
            match context.step_output_as::<CleanupDispatchStepOutput>(&dispatch_step_id)? {
                Some(CleanupDispatchStepOutput::NotRequired { cleaned_at }) => {
                    return complete_cancellation_command(config, context, flow_input, cleaned_at)
                }
                Some(CleanupDispatchStepOutput::Ready { dispatched }) => dispatched,
                Some(CleanupDispatchStepOutput::Retry {
                    next_attempt_at,
                    deadline_at,
                    ..
                }) => {
                    validate_cleanup_retry(next_attempt_at, deadline_at)?;
                    let wait_id = format!("cleanup-dispatch-retry-wait-{attempt}");
                    if !context.wait_completed(&wait_id) {
                        return Ok(context.wait_until(wait_id, next_attempt_at));
                    }
                    attempt = next_cleanup_attempt(attempt)?;
                    issued_at = Some(next_attempt_at);
                    continue;
                }
                Some(CleanupDispatchStepOutput::Failed { reason }) => {
                    return failure_command(config, context, flow_input, reason)
                }
                None => {
                    return stage_or_failure(
                        config,
                        context,
                        flow_input,
                        &dispatch_step_id,
                        "dispatch_runtime_cleanup",
                        &CleanupDispatchStepInput {
                            resolved: resolved.clone(),
                            attempt,
                            issued_at,
                        },
                    )
                }
            };
        if dispatched.attempt != attempt {
            return Err(FlowError::Runtime(
                "Runtime cleanup dispatch changed its attempt".into(),
            ));
        }
        match observe_cleanup(config, context, flow_input, resolved, &dispatched)? {
            CleanupProgress::Ready(cleaned_at) => {
                return complete_cancellation_command(config, context, flow_input, cleaned_at)
            }
            CleanupProgress::Retry {
                next_attempt_at,
                deadline_at,
            } => {
                validate_cleanup_retry(next_attempt_at, deadline_at)?;
                let wait_id = format!("cleanup-observe-retry-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_attempt_at));
                }
                attempt = next_cleanup_attempt(attempt)?;
                issued_at = Some(next_attempt_at);
            }
            CleanupProgress::Command(command) => return Ok(command),
        }
    }
}

fn observe_cleanup(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    resolved: &ResolveStepOutput,
    dispatched: &DispatchedCleanup,
) -> a3s_flow::Result<CleanupProgress> {
    let mut poll = 1_u32;
    loop {
        let step_id = format!("cleanup-observe-{}-{poll}", dispatched.attempt);
        match context.step_output_as::<CleanupObserveStepOutput>(&step_id)? {
            Some(CleanupObserveStepOutput::Ready { cleaned_at }) => {
                return Ok(CleanupProgress::Ready(cleaned_at))
            }
            Some(CleanupObserveStepOutput::Retry {
                next_attempt_at,
                deadline_at,
                ..
            }) => {
                return Ok(CleanupProgress::Retry {
                    next_attempt_at,
                    deadline_at,
                })
            }
            Some(CleanupObserveStepOutput::Failed { reason }) => {
                return failure_command(config, context, flow_input, reason)
                    .map(CleanupProgress::Command)
            }
            Some(CleanupObserveStepOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                if next_poll_at > deadline_at {
                    return Err(FlowError::Runtime(
                        "cleanup observation poll exceeds its attempt deadline".into(),
                    ));
                }
                let wait_id = format!("cleanup-observe-wait-{}-{poll}", dispatched.attempt);
                if !context.wait_completed(&wait_id) {
                    return Ok(CleanupProgress::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                poll = poll
                    .checked_add(1)
                    .ok_or_else(|| FlowError::Runtime("cleanup poll overflowed".into()))?;
            }
            None => {
                let command = stage_or_failure(
                    config,
                    context,
                    flow_input,
                    &step_id,
                    "observe_runtime_cleanup",
                    &CleanupObserveStepInput {
                        resolved: resolved.clone(),
                        dispatched: dispatched.clone(),
                    },
                )?;
                return Ok(CleanupProgress::Command(command));
            }
        }
    }
}

fn complete_cancellation_command(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    cleaned_at: chrono::DateTime<chrono::Utc>,
) -> a3s_flow::Result<RuntimeCommand> {
    match context.step_output_as::<CompleteCancellationStepOutput>(COMPLETE_CANCELLATION_STEP_ID)? {
        Some(output) => Ok(context.complete(serde_json::to_value(output)?)),
        None => stage_or_failure(
            config,
            context,
            flow_input,
            COMPLETE_CANCELLATION_STEP_ID,
            "complete_deployment_cancellation",
            &CompleteCancellationStepInput {
                deployment_id: flow_input.deployment_id,
                organization_id: flow_input.organization_id,
                cleaned_at,
            },
        ),
    }
}

fn validate_cleanup_retry(
    next_attempt_at: chrono::DateTime<chrono::Utc>,
    deadline_at: chrono::DateTime<chrono::Utc>,
) -> a3s_flow::Result<()> {
    if next_attempt_at > deadline_at {
        return Err(FlowError::Runtime(
            "cleanup retry exceeds its independent deadline".into(),
        ));
    }
    Ok(())
}

fn next_cleanup_attempt(attempt: u32) -> a3s_flow::Result<u32> {
    attempt
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime("cleanup attempt overflowed".into()))
}

fn stage_or_failure<T: serde::Serialize>(
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return failure_command(
            config,
            context,
            flow_input,
            format!("deployment stage {step_name} failed: {error}"),
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
    config: &DeploymentFlowConfig,
    context: &WorkflowContext<'_>,
    flow_input: &DeploymentFlowInput,
    reason: String,
) -> a3s_flow::Result<RuntimeCommand> {
    if context.step_failed(FAIL_STEP_ID).is_some() {
        return Err(FlowError::Runtime(
            "deployment failure could not be persisted".into(),
        ));
    }
    Ok(context.schedule_step_with_retry(
        FAIL_STEP_ID,
        "fail_deployment",
        serde_json::to_value(FailStepInput {
            deployment_id: flow_input.deployment_id,
            organization_id: flow_input.organization_id,
            reason,
        })?,
        config.retry_policy(),
    ))
}

enum Progress<T> {
    Ready(T),
    Cancellation,
    Command(RuntimeCommand),
}

enum CleanupProgress {
    Ready(chrono::DateTime<chrono::Utc>),
    Retry {
        next_attempt_at: chrono::DateTime<chrono::Utc>,
        deadline_at: chrono::DateTime<chrono::Utc>,
    },
    Command(RuntimeCommand),
}
