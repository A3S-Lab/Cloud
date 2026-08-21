mod runtime;
mod types;
mod workflow;

#[cfg(test)]
mod tests;

use crate::infrastructure::flow_step_retry_policy;
use crate::modules::agents::domain::IAgentRepository;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::workloads::domain::repositories::IWorkloadRuntimeTargetRepository;
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

const AGENT_EXECUTION_PREPARE: &str = "agent_execution_prepare";
const AGENT_EXECUTION_DISPATCH: &str = "agent_execution_dispatch";
const AGENT_EXECUTION_OBSERVE: &str = "agent_execution_observe";
const STEP_NAMES: &[&str] = &[
    AGENT_EXECUTION_PREPARE,
    AGENT_EXECUTION_DISPATCH,
    AGENT_EXECUTION_OBSERVE,
];

#[derive(Debug, Clone, Copy)]
pub struct AgentExecutionFlowConfigOptions {
    pub heartbeat_timeout_ms: u64,
    pub command_ttl_ms: u64,
    pub observation_poll_ms: u64,
    pub convergence_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AgentExecutionFlowConfig {
    pub heartbeat_timeout: chrono::Duration,
    pub command_ttl: chrono::Duration,
    pub observation_poll: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    retry_delay: Duration,
}

impl AgentExecutionFlowConfig {
    pub fn new(options: AgentExecutionFlowConfigOptions) -> Result<Self, String> {
        if [
            options.heartbeat_timeout_ms,
            options.command_ttl_ms,
            options.observation_poll_ms,
            options.convergence_timeout_ms,
        ]
        .contains(&0)
            || options.observation_poll_ms > options.convergence_timeout_ms
        {
            return Err("Agent execution Flow configuration is invalid".into());
        }
        Ok(Self {
            heartbeat_timeout: duration(options.heartbeat_timeout_ms)?,
            command_ttl: duration(options.command_ttl_ms)?,
            observation_poll: duration(options.observation_poll_ms)?,
            convergence_timeout: duration(options.convergence_timeout_ms)?,
            retry_delay: Duration::from_millis(options.observation_poll_ms),
        })
    }

    fn retry_policy(&self, context: &WorkflowContext<'_>) -> a3s_flow::RetryPolicy {
        flow_step_retry_policy(context, self.retry_delay)
    }
}

fn duration(milliseconds: u64) -> Result<chrono::Duration, String> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| "Agent execution Flow duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct AgentExecutionFlowRuntimeDependencies {
    pub agents: Arc<dyn IAgentRepository>,
    pub workload_targets: Arc<dyn IWorkloadRuntimeTargetRepository>,
    pub node_control: Arc<dyn INodeControlRepository>,
}

#[derive(Clone)]
pub struct AgentExecutionFlowRuntime {
    agents: Arc<dyn IAgentRepository>,
    workload_targets: Arc<dyn IWorkloadRuntimeTargetRepository>,
    node_control: Arc<dyn INodeControlRepository>,
    config: AgentExecutionFlowConfig,
}

impl AgentExecutionFlowRuntime {
    pub fn new(
        dependencies: AgentExecutionFlowRuntimeDependencies,
        config: AgentExecutionFlowConfig,
    ) -> Self {
        Self {
            agents: dependencies.agents,
            workload_targets: dependencies.workload_targets,
            node_control: dependencies.node_control,
            config,
        }
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    STEP_NAMES.iter().copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once((
        crate::modules::agents::application::AGENT_EXECUTION_WORKFLOW_NAME,
        crate::modules::agents::application::AGENT_EXECUTION_WORKFLOW_VERSION,
    ))
}

#[async_trait]
impl FlowRuntime for AgentExecutionFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        workflow::replay(&self.config, invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            AGENT_EXECUTION_PREPARE => {
                encode(runtime::prepare(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            AGENT_EXECUTION_DISPATCH => {
                encode(runtime::dispatch(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            AGENT_EXECUTION_OBSERVE => {
                encode(runtime::observe(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            step => Err(FlowError::Runtime(format!(
                "Cloud Agent execution workflow has no step {step:?}"
            ))),
        }
    }
}

fn encode<T: serde::Serialize>(value: T) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(FlowError::from)
}

fn flow_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}
