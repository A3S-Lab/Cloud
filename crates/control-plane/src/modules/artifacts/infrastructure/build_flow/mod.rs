mod build_plan;
mod steps;
mod types;
mod workflow;

#[cfg(test)]
mod tests;

use crate::infrastructure::flow_step_retry_policy;
use crate::modules::artifacts::application::IBuildInputPreparer;
use crate::modules::artifacts::domain::{
    IBuildArtifactPublisher, IBuildEvidenceGenerator, IBuildOutputValidator, IBuildRunRepository,
    IBuildSourceResolver,
};
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BuildFlowConfigOptions {
    pub heartbeat_timeout_ms: u64,
    pub command_ttl_ms: u64,
    pub execution_timeout_ms: u64,
    pub observation_poll_ms: u64,
    pub convergence_timeout_ms: u64,
    pub cleanup_timeout_ms: u64,
    pub publication_timeout_ms: u64,
    pub output_max_bytes: u64,
    pub cache_max_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct BuildFlowConfig {
    pub heartbeat_timeout: chrono::Duration,
    pub command_ttl: chrono::Duration,
    pub execution_timeout: chrono::Duration,
    pub observation_poll: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    pub cleanup_timeout: chrono::Duration,
    pub publication_timeout: chrono::Duration,
    pub output_max_bytes: u64,
    pub cache_max_bytes: u64,
    retry_delay: Duration,
}

impl BuildFlowConfig {
    pub fn new(options: BuildFlowConfigOptions) -> Result<Self, String> {
        let BuildFlowConfigOptions {
            heartbeat_timeout_ms,
            command_ttl_ms,
            execution_timeout_ms,
            observation_poll_ms,
            convergence_timeout_ms,
            cleanup_timeout_ms,
            publication_timeout_ms,
            output_max_bytes,
            cache_max_bytes,
        } = options;
        if [
            heartbeat_timeout_ms,
            command_ttl_ms,
            execution_timeout_ms,
            observation_poll_ms,
            convergence_timeout_ms,
            cleanup_timeout_ms,
            publication_timeout_ms,
            output_max_bytes,
            cache_max_bytes,
        ]
        .contains(&0)
            || command_ttl_ms < execution_timeout_ms
            || convergence_timeout_ms < execution_timeout_ms
        {
            return Err("build Flow configuration is invalid".into());
        }
        Ok(Self {
            heartbeat_timeout: chrono_duration(heartbeat_timeout_ms)?,
            command_ttl: chrono_duration(command_ttl_ms)?,
            execution_timeout: chrono_duration(execution_timeout_ms)?,
            observation_poll: chrono_duration(observation_poll_ms)?,
            convergence_timeout: chrono_duration(convergence_timeout_ms)?,
            cleanup_timeout: chrono_duration(cleanup_timeout_ms)?,
            publication_timeout: chrono_duration(publication_timeout_ms)?,
            output_max_bytes,
            cache_max_bytes,
            retry_delay: Duration::from_millis(observation_poll_ms.min(cleanup_timeout_ms)),
        })
    }

    pub(super) fn retry_policy(&self, context: &WorkflowContext<'_>) -> a3s_flow::RetryPolicy {
        flow_step_retry_policy(context, self.retry_delay)
    }
}

fn chrono_duration(milliseconds: u64) -> Result<chrono::Duration, String> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| "build Flow duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct BuildFlowRuntimeDependencies {
    pub builds: Arc<dyn IBuildRunRepository>,
    pub sources: Arc<dyn IBuildSourceResolver>,
    pub inputs: Arc<dyn IBuildInputPreparer>,
    pub outputs: Arc<dyn IBuildOutputValidator>,
    pub publisher: Arc<dyn IBuildArtifactPublisher>,
    pub evidence: Arc<dyn IBuildEvidenceGenerator>,
    pub nodes: Arc<dyn INodeRepository>,
    pub node_control: Arc<dyn INodeControlRepository>,
}

#[derive(Clone)]
pub struct BuildFlowRuntime {
    pub(super) builds: Arc<dyn IBuildRunRepository>,
    pub(super) sources: Arc<dyn IBuildSourceResolver>,
    pub(super) inputs: Arc<dyn IBuildInputPreparer>,
    pub(super) outputs: Arc<dyn IBuildOutputValidator>,
    pub(super) publisher: Arc<dyn IBuildArtifactPublisher>,
    pub(super) evidence: Arc<dyn IBuildEvidenceGenerator>,
    pub(super) nodes: Arc<dyn INodeRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) config: BuildFlowConfig,
}

impl BuildFlowRuntime {
    pub fn new(dependencies: BuildFlowRuntimeDependencies, config: BuildFlowConfig) -> Self {
        let BuildFlowRuntimeDependencies {
            builds,
            sources,
            inputs,
            outputs,
            publisher,
            evidence,
            nodes,
            node_control,
        } = dependencies;
        Self {
            builds,
            sources,
            inputs,
            outputs,
            publisher,
            evidence,
            nodes,
            node_control,
            config,
        }
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    steps::STEP_NAMES.iter().copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once((
        crate::modules::artifacts::application::BUILD_WORKFLOW_NAME,
        crate::modules::artifacts::application::BUILD_WORKFLOW_VERSION,
    ))
}

#[async_trait]
impl FlowRuntime for BuildFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        workflow::replay(&self.config, invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        steps::execute(self, invocation).await
    }
}

fn flow_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}
