mod steps;
mod stop_workflow;
#[cfg(test)]
mod tests;
mod types;
mod workflow;

use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::domain::services::IOciArtifactResolver;
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub const DEPLOYMENT_WORKFLOW_NAME: &str = "cloud.deployment";
pub const DEPLOYMENT_WORKFLOW_VERSION: &str = "1";
pub const STOP_WORKFLOW_NAME: &str = "cloud.workload.stop";
pub const STOP_WORKFLOW_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub struct DeploymentFlowConfig {
    pub command_ttl: chrono::Duration,
    pub runtime_apply_timeout: chrono::Duration,
    pub observation_poll: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    pub runtime_stop_timeout: chrono::Duration,
    pub cleanup_poll: chrono::Duration,
    pub cleanup_timeout: chrono::Duration,
    retry_delay: Duration,
}

impl DeploymentFlowConfig {
    pub fn from_milliseconds(
        command_ttl_ms: u64,
        runtime_apply_timeout_ms: u64,
        observation_poll_ms: u64,
        convergence_timeout_ms: u64,
        runtime_stop_timeout_ms: u64,
        cleanup_poll_ms: u64,
        cleanup_timeout_ms: u64,
    ) -> Result<Self, String> {
        if [
            command_ttl_ms,
            runtime_apply_timeout_ms,
            observation_poll_ms,
            convergence_timeout_ms,
            runtime_stop_timeout_ms,
            cleanup_poll_ms,
            cleanup_timeout_ms,
        ]
        .contains(&0)
        {
            return Err(
                "deployment apply, command, observation, convergence, stop, and cleanup timings must each be positive"
                    .into(),
            );
        }
        Ok(Self {
            command_ttl: chrono_duration(command_ttl_ms)?,
            runtime_apply_timeout: chrono_duration(runtime_apply_timeout_ms)?,
            observation_poll: chrono_duration(observation_poll_ms)?,
            convergence_timeout: chrono_duration(convergence_timeout_ms)?,
            runtime_stop_timeout: chrono_duration(runtime_stop_timeout_ms)?,
            cleanup_poll: chrono_duration(cleanup_poll_ms)?,
            cleanup_timeout: chrono_duration(cleanup_timeout_ms)?,
            retry_delay: Duration::from_millis(observation_poll_ms.min(cleanup_poll_ms)),
        })
    }

    pub(super) fn retry_policy(&self) -> a3s_flow::RetryPolicy {
        // Infrastructure failures keep the durable operation suspended. Business
        // failures are returned as typed step output and persisted by fail_deployment.
        a3s_flow::RetryPolicy::fixed(u32::MAX, self.retry_delay)
    }
}

fn chrono_duration(milliseconds: u64) -> Result<chrono::Duration, String> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| "deployment duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct DeploymentFlowRuntime {
    pub(super) workloads: Arc<dyn IWorkloadRepository>,
    pub(super) artifacts: Arc<dyn IOciArtifactResolver>,
    pub(super) nodes: Arc<dyn INodeRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) heartbeat_timeout: chrono::Duration,
    pub(super) config: DeploymentFlowConfig,
}

impl DeploymentFlowRuntime {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        artifacts: Arc<dyn IOciArtifactResolver>,
        nodes: Arc<dyn INodeRepository>,
        node_control: Arc<dyn INodeControlRepository>,
        heartbeat_timeout: chrono::Duration,
        config: DeploymentFlowConfig,
    ) -> Result<Self, String> {
        if heartbeat_timeout <= chrono::Duration::zero() {
            return Err("deployment scheduler heartbeat timeout must be positive".into());
        }
        Ok(Self {
            workloads,
            artifacts,
            nodes,
            node_control,
            heartbeat_timeout,
            config,
        })
    }
}

#[async_trait]
impl FlowRuntime for DeploymentFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        match (
            invocation.spec.name.as_str(),
            invocation.spec.version.as_str(),
        ) {
            (DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION) => {
                workflow::replay(&self.config, invocation)
            }
            (STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION) => {
                stop_workflow::replay(&self.config, invocation)
            }
            _ => Err(FlowError::Runtime(format!(
                "Cloud has no workflow runtime for {}@{}",
                invocation.spec.name, invocation.spec.version
            ))),
        }
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        if invocation.step_name.starts_with("stop_workload_") {
            stop_workflow::execute(self, invocation).await
        } else {
            steps::execute(self, invocation).await
        }
    }
}

fn flow_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}
