use super::types::{
    AgentExecutionFlowInput, CompletedAgentExecution, DispatchInput, DispatchOutput, ObserveInput,
    ObserveOutput, PrepareOutput, PreparedAgentExecution,
};
use super::{
    AgentExecutionFlowConfig, AGENT_EXECUTION_DISPATCH, AGENT_EXECUTION_OBSERVE,
    AGENT_EXECUTION_PREPARE,
};
use crate::modules::agents::application::{
    AGENT_EXECUTION_WORKFLOW_NAME, AGENT_EXECUTION_WORKFLOW_VERSION,
};
use a3s_flow::{FlowError, RuntimeCommand, WorkflowContext, WorkflowInvocation};
use serde::Serialize;

pub(super) fn replay(
    config: &AgentExecutionFlowConfig,
    invocation: WorkflowInvocation,
) -> a3s_flow::Result<RuntimeCommand> {
    if invocation.spec.name != AGENT_EXECUTION_WORKFLOW_NAME
        || invocation.spec.version != AGENT_EXECUTION_WORKFLOW_VERSION
    {
        return Err(FlowError::Runtime(format!(
            "Cloud has no Agent execution workflow runtime for {}@{}",
            invocation.spec.name, invocation.spec.version
        )));
    }
    let context = invocation.context();
    let input = context.input_as::<AgentExecutionFlowInput>()?;
    let prepared = match prepare(config, &context, &input)? {
        Progress::Ready(prepared) => prepared,
        Progress::Terminal(completed) => return complete(&context, completed),
        Progress::Command(command) => return Ok(command),
    };
    let dispatched = match context.step_output_as::<DispatchOutput>("dispatch")? {
        Some(DispatchOutput::Ready { dispatched }) => *dispatched,
        Some(DispatchOutput::Terminal { completed }) => return complete(&context, completed),
        None => {
            return stage(
                config,
                &context,
                "dispatch",
                AGENT_EXECUTION_DISPATCH,
                &DispatchInput {
                    prepared: Box::new(prepared),
                },
            )
        }
    };
    observe(config, &context, dispatched)
}

fn prepare(
    config: &AgentExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    input: &AgentExecutionFlowInput,
) -> a3s_flow::Result<Progress<PreparedAgentExecution>> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("prepare-{attempt}");
        match context.step_output_as::<PrepareOutput>(&step_id)? {
            Some(PrepareOutput::Ready { prepared }) => return Ok(Progress::Ready(*prepared)),
            Some(PrepareOutput::Terminal { completed }) => {
                return Ok(Progress::Terminal(completed))
            }
            Some(PrepareOutput::Pending {
                next_poll_at,
                deadline_at,
                ..
            }) => {
                if next_poll_at > deadline_at {
                    return Err(FlowError::Runtime(
                        "Agent execution preparation poll exceeds its deadline".into(),
                    ));
                }
                let wait_id = format!("prepare-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(Progress::Command(context.wait_until(wait_id, next_poll_at)));
                }
                attempt = next_attempt(attempt)?;
            }
            None => {
                return stage(config, context, &step_id, AGENT_EXECUTION_PREPARE, input)
                    .map(Progress::Command)
            }
        }
    }
}

fn observe(
    config: &AgentExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    dispatched: super::types::DispatchedAgentExecution,
) -> a3s_flow::Result<RuntimeCommand> {
    let mut attempt = 1_u32;
    loop {
        let step_id = format!("observe-{attempt}");
        match context.step_output_as::<ObserveOutput>(&step_id)? {
            Some(ObserveOutput::Terminal { completed }) => return complete(context, completed),
            Some(ObserveOutput::Pending { next_poll_at, .. }) => {
                let wait_id = format!("observe-wait-{attempt}");
                if !context.wait_completed(&wait_id) {
                    return Ok(context.wait_until(wait_id, next_poll_at));
                }
                attempt = next_attempt(attempt)?;
            }
            None => {
                return stage(
                    config,
                    context,
                    &step_id,
                    AGENT_EXECUTION_OBSERVE,
                    &ObserveInput {
                        dispatched: Box::new(dispatched),
                    },
                )
            }
        }
    }
}

fn stage<T: Serialize>(
    config: &AgentExecutionFlowConfig,
    context: &WorkflowContext<'_>,
    step_id: &str,
    step_name: &str,
    input: &T,
) -> a3s_flow::Result<RuntimeCommand> {
    if let Some(error) = context.step_failed(step_id) {
        return Ok(context.fail(format!("Agent execution stage {step_name} failed: {error}")));
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
    completed: CompletedAgentExecution,
) -> a3s_flow::Result<RuntimeCommand> {
    Ok(context.complete(serde_json::to_value(completed)?))
}

fn next_attempt(value: u32) -> a3s_flow::Result<u32> {
    value
        .checked_add(1)
        .ok_or_else(|| FlowError::Runtime("Agent execution Flow attempt overflowed".into()))
}

// This short-lived control result returns RuntimeCommand immediately; boxing it
// would add a heap allocation to every dispatch or wait transition.
#[allow(clippy::large_enum_variant)]
enum Progress<T> {
    Ready(T),
    Terminal(CompletedAgentExecution),
    Command(RuntimeCommand),
}
