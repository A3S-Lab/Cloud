use super::types::{
    CleanupDispatchInput, CleanupDispatchOutput, CleanupObserveInput, CleanupObserveOutput,
    CompletedExecution, DispatchInput, DispatchOutput, ExecutionFlowInput, ObserveInput,
    ObserveOutput, ScheduleOutput, ScheduledExecution, TerminalExecution,
};
use super::ExecutionFlowConfig;
use crate::modules::executions::application::{
    EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};
use chrono::{DateTime, Utc};
use serde::Serialize;

const DISPATCH_STEP_ID: &str = "dispatch";

pub(super) fn replay(
    config: &ExecutionFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    if invocation.spec.name != EXECUTION_WORKFLOW_NAME
        || invocation.spec.version != EXECUTION_WORKFLOW_VERSION
    {
        return Err(FlowError::Runtime(format!(
            "Cloud has no execution workflow runtime for {}@{}",
            invocation.spec.name, invocation.spec.version
        )));
    }
    let context = invocation.context();
    let input = context.input_as::<ExecutionFlowInput>()?;
    let scheduled = match schedule(config, &context, &input)? {
        Progress::Ready(scheduled) => scheduled,
        Progress::Terminal(terminal) => return cleanup(config, &context, terminal),
        Progress::Command(command) => return Ok(command),
    };
    let dispatched = match context.step_output_as::<DispatchOutput>(DISPATCH_STEP_ID)? {
        Some(DispatchOutput::Ready { dispatched }) => *dispatched,
        Some(DispatchOutput::Terminal { terminal }) => return cleanup(config, &context, terminal),
        None => {
            return stage(
                config,
                &context,
                DISPATCH_STEP_ID,
                "execution_dispatch_runtime",
                &DispatchInput {
                    scheduled: Box::new(scheduled),
                },
            )
        }
    };
    match observe(config, &context, dispatched)? {
        ObserveProgress::Terminal(terminal) => cleanup(config, &context, terminal),
        ObserveProgress::Command(command) => Ok(command),
    }
}

fn schedule(
    config: &ExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    input: &ExecutionFlowInput,
) -> a3s_flow::Result<Progress<ScheduledExecution>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("schedule-{attempt}");
        match context.step_output_as::<ScheduleOutput>(&step_id)? {
            Some(ScheduleOutput::Ready { scheduled }) => return Ok(Progress::Ready(*scheduled)),
            Some(ScheduleOutput::Terminal { terminal }) => return Ok(Progress::Terminal(terminal)),
            Some(ScheduleOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("execution scheduling", next_poll_at, deadline_at)?;
                let wait_id = format!("schedule-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt, "execution scheduling")?;
            }
            None => {
                return stage(
                    config,
                    context,
                    &step_id,
                    "execution_schedule_runtime",
                    input,
                )
                .map(Progress::Command)
            }
        }
    }
}

fn observe(
    config: &ExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    dispatched: super::types::DispatchedExecution,
) -> a3s_flow::Result<ObserveProgress> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("observe-{attempt}");
        match context.step_output_as::<ObserveOutput>(&step_id)? {
            Some(ObserveOutput::Terminal { terminal }) => {
                return Ok(ObserveProgress::Terminal(terminal))
            }
            Some(ObserveOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                validate_poll("execution observation", next_poll_at, deadline_at)?;
                if deadline_at != dispatched.result_deadline {
                    return Err(FlowError::Runtime(
                        "execution observation changed its deadline".into(),
                    ));
                }
                let wait_id = format!("observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(ObserveProgress::Command(
                        context.wait_until(wait_id, next_poll_at),
                    ));
                }
                attempt = next_attempt(attempt, "execution observation")?;
            }
            None => {
                return stage(
                    config,
                    context,
                    &step_id,
                    "execution_observe_runtime",
                    &ObserveInput {
                        dispatched: Box::new(dispatched),
                    },
                )
                .map(ObserveProgress::Command)
            }
        }
    }
}

fn cleanup(
    config: &ExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    terminal: TerminalExecution,
) -> a3s_flow::Result<RuntimeCommand> {
    let cleanup_deadline = terminal
        .terminal_at
        .checked_add_signed(config.cleanup_timeout)
        .ok_or_else(|| FlowError::Runtime("execution cleanup deadline overflowed".into()))?;
    let mut attempt = 1_u32;
    let mut issued_at = terminal.terminal_at;
    loop {
        let dispatch_id = format!("cleanup-dispatch-{attempt}");
        let dispatched = match context.step_output_as::<CleanupDispatchOutput>(&dispatch_id)? {
            Some(CleanupDispatchOutput::Completed { execution }) => {
                return complete(context, execution)
            }
            Some(CleanupDispatchOutput::Ready { dispatched }) => dispatched,
            Some(CleanupDispatchOutput::Retry {
                next_attempt_at,
                deadline_at,
                ..
            }) => {
                validate_cleanup_retry(cleanup_deadline, next_attempt_at, deadline_at)?;
                let wait_id = format!("cleanup-dispatch-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_attempt_at));
                }
                attempt = next_attempt(attempt, "execution cleanup")?;
                issued_at = next_attempt_at;
                continue;
            }
            None => {
                return stage(
                    config,
                    context,
                    &dispatch_id,
                    "execution_cleanup_dispatch",
                    &CleanupDispatchInput {
                        terminal: terminal.clone(),
                        attempt,
                        issued_at,
                        cleanup_deadline,
                    },
                )
            }
        };
        let mut poll = 1_u32;
        loop {
            let observe_id = format!("cleanup-observe-{attempt}-{poll}");
            match context.step_output_as::<CleanupObserveOutput>(&observe_id)? {
                Some(CleanupObserveOutput::Completed { execution }) => {
                    return complete(context, execution)
                }
                Some(CleanupObserveOutput::Pending {
                    next_poll_at,
                    deadline_at,
                    ..
                }) => {
                    validate_poll("execution cleanup observation", next_poll_at, deadline_at)?;
                    if deadline_at != dispatched.result_deadline {
                        return Err(FlowError::Runtime(
                            "execution cleanup observation changed its attempt deadline".into(),
                        ));
                    }
                    let wait_id = format!("cleanup-observe-wait-{attempt}-{poll}");
                    if !context.wait_completed(&wait_id) {
                        return Ok(context.wait_until(wait_id, next_poll_at));
                    }
                    poll = next_attempt(poll, "execution cleanup poll")?;
                }
                Some(CleanupObserveOutput::Retry {
                    next_attempt_at,
                    deadline_at,
                    ..
                }) => {
                    validate_cleanup_retry(cleanup_deadline, next_attempt_at, deadline_at)?;
                    let wait_id = format!("cleanup-retry-wait-{attempt}");
                    if !context.wait_completed(&wait_id) {
                        return Ok(context.wait_until(wait_id, next_attempt_at));
                    }
                    attempt = next_attempt(attempt, "execution cleanup")?;
                    issued_at = next_attempt_at;
                    break;
                }
                None => {
                    return stage(
                        config,
                        context,
                        &observe_id,
                        "execution_cleanup_observe",
                        &CleanupObserveInput {
                            dispatched: dispatched.clone(),
                        },
                    )
                }
            }
        }
    }
}

fn stage<T: Serialize>(
    config: &ExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return Ok(context.fail(format!("execution stage {step_name} failed: {error}")));
    }
    Ok(context.schedule_step_with_retry(
        step_id,
        step_name,
        serde_json::to_value(input)?,
        config.retry_policy(),
    ))
}

fn complete(
    context: &WorkflowContext<'_>,
    execution: CompletedExecution,
) -> a3s_flow::Result<RuntimeCommand> {
    Ok(context.complete(serde_json::to_value(execution)?))
}

fn validate_poll(
    label: &str,
    next_poll_at: DateTime<Utc>,
    deadline_at: DateTime<Utc>,
) -> a3s_flow::Result<()> {
    if next_poll_at > deadline_at {
        return Err(FlowError::Runtime(format!(
            "{label} poll exceeds its deadline"
        )));
    }
    Ok(())
}

fn validate_cleanup_retry(
    expected_deadline: DateTime<Utc>,
    next_attempt_at: DateTime<Utc>,
    deadline_at: DateTime<Utc>,
) -> a3s_flow::Result<()> {
    if deadline_at != expected_deadline || next_attempt_at > deadline_at {
        return Err(FlowError::Runtime(
            "execution cleanup retry changed its deadline".into(),
        ));
    }
    Ok(())
}

fn next_attempt(value: u32, label: &str) -> a3s_flow::Result<u32> {
    value
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime(format!("{label} attempt overflowed")))
}

enum Progress<T> {
    Ready(T),
    Terminal(TerminalExecution),
    Command(RuntimeCommand),
}

enum ObserveProgress {
    Terminal(TerminalExecution),
    Command(RuntimeCommand),
}
