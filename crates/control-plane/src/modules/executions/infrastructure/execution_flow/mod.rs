mod cleanup;
mod common;
mod runtime;
mod types;
mod validation;
mod workflow;

#[cfg(test)]
mod tests;

use crate::infrastructure::flow_step_retry_policy;
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

const EXECUTION_SCHEDULE_RUNTIME: &str = "execution_schedule_runtime";
const EXECUTION_DISPATCH_RUNTIME: &str = "execution_dispatch_runtime";
const EXECUTION_OBSERVE_RUNTIME: &str = "execution_observe_runtime";
const EXECUTION_CLEANUP_DISPATCH: &str = "execution_cleanup_dispatch";
const EXECUTION_CLEANUP_OBSERVE: &str = "execution_cleanup_observe";
const STEP_NAMES: &[&str] = &[
    EXECUTION_SCHEDULE_RUNTIME,
    EXECUTION_DISPATCH_RUNTIME,
    EXECUTION_OBSERVE_RUNTIME,
    EXECUTION_CLEANUP_DISPATCH,
    EXECUTION_CLEANUP_OBSERVE,
];

#[derive(Debug, Clone, Copy)]
pub struct ExecutionFlowConfigOptions {
    pub heartbeat_timeout_ms: u64,
    pub command_ttl_ms: u64,
    pub observation_poll_ms: u64,
    pub convergence_timeout_ms: u64,
    pub cleanup_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionFlowConfig {
    pub heartbeat_timeout: chrono::Duration,
    pub command_ttl: chrono::Duration,
    pub observation_poll: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    pub cleanup_timeout: chrono::Duration,
    retry_delay: Duration,
}

impl ExecutionFlowConfig {
    pub fn new(options: ExecutionFlowConfigOptions) -> Result<Self, String> {
        if [
            options.heartbeat_timeout_ms,
            options.command_ttl_ms,
            options.observation_poll_ms,
            options.convergence_timeout_ms,
            options.cleanup_timeout_ms,
        ]
        .contains(&0)
            || options.command_ttl_ms < crate::modules::executions::domain::MAX_EXECUTION_TIMEOUT_MS
            || options.observation_poll_ms > options.convergence_timeout_ms
            || options.observation_poll_ms > options.cleanup_timeout_ms
        {
            return Err("execution Flow configuration is invalid".into());
        }
        Ok(Self {
            heartbeat_timeout: duration(options.heartbeat_timeout_ms)?,
            command_ttl: duration(options.command_ttl_ms)?,
            observation_poll: duration(options.observation_poll_ms)?,
            convergence_timeout: duration(options.convergence_timeout_ms)?,
            cleanup_timeout: duration(options.cleanup_timeout_ms)?,
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
        .map_err(|_| "execution Flow duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct ExecutionFlowRuntimeDependencies {
    pub executions: Arc<dyn IExecutionRepository>,
    pub nodes: Arc<dyn INodeRepository>,
    pub node_control: Arc<dyn INodeControlRepository>,
}

#[derive(Clone)]
pub struct ExecutionFlowRuntime {
    pub(super) executions: Arc<dyn IExecutionRepository>,
    pub(super) nodes: Arc<dyn INodeRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) config: ExecutionFlowConfig,
}

impl ExecutionFlowRuntime {
    pub fn new(
        dependencies: ExecutionFlowRuntimeDependencies,
        config: ExecutionFlowConfig,
    ) -> Self {
        Self {
            executions: dependencies.executions,
            nodes: dependencies.nodes,
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
        crate::modules::executions::application::EXECUTION_WORKFLOW_NAME,
        crate::modules::executions::application::EXECUTION_WORKFLOW_VERSION,
    ))
}

#[async_trait]
impl FlowRuntime for ExecutionFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        workflow::replay(&self.config, invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            EXECUTION_SCHEDULE_RUNTIME => {
                encode(runtime::schedule(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            EXECUTION_DISPATCH_RUNTIME => {
                encode(runtime::dispatch(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            EXECUTION_OBSERVE_RUNTIME => {
                encode(runtime::observe(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            EXECUTION_CLEANUP_DISPATCH => {
                encode(cleanup::dispatch(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            EXECUTION_CLEANUP_OBSERVE => {
                encode(cleanup::observe(self, &invocation.run_id, invocation.input_as()?).await?)
            }
            step => Err(FlowError::Runtime(format!(
                "Cloud execution workflow has no step {step:?}"
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
