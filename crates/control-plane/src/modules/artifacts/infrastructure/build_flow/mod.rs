mod steps;
mod task_spec;
mod types;
mod workflow;

#[cfg(test)]
mod tests;

use crate::modules::artifacts::domain::{
    IBuildArtifactPublisher, IBuildEvidenceGenerator, IBuildInputPreparer, IBuildOutputValidator,
    IBuildRunRepository,
};
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use crate::modules::sources::domain::ISourceRevisionRepository;
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct BuildFlowConfigOptions {
    pub builder: ArtifactRef,
    pub buildkit_socket_volume_id: String,
    pub heartbeat_timeout_ms: u64,
    pub command_ttl_ms: u64,
    pub execution_timeout_ms: u64,
    pub observation_poll_ms: u64,
    pub convergence_timeout_ms: u64,
    pub cleanup_timeout_ms: u64,
    pub publication_timeout_ms: u64,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub output_max_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct BuildFlowConfig {
    pub builder: ArtifactRef,
    pub buildkit_socket_volume_id: String,
    pub heartbeat_timeout: chrono::Duration,
    pub command_ttl: chrono::Duration,
    pub execution_timeout: chrono::Duration,
    pub observation_poll: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    pub cleanup_timeout: chrono::Duration,
    pub publication_timeout: chrono::Duration,
    pub cpu_millis: u64,
    pub memory_bytes: u64,
    pub pids: u32,
    pub output_max_bytes: u64,
    retry_delay: Duration,
}

impl BuildFlowConfig {
    pub fn new(options: BuildFlowConfigOptions) -> Result<Self, String> {
        let BuildFlowConfigOptions {
            builder,
            buildkit_socket_volume_id,
            heartbeat_timeout_ms,
            command_ttl_ms,
            execution_timeout_ms,
            observation_poll_ms,
            convergence_timeout_ms,
            cleanup_timeout_ms,
            publication_timeout_ms,
            cpu_millis,
            memory_bytes,
            pids,
            output_max_bytes,
        } = options;
        validate_builder(&builder)?;
        if buildkit_socket_volume_id.trim().is_empty()
            || buildkit_socket_volume_id.len() > 255
            || buildkit_socket_volume_id.contains(['\0', '\r', '\n'])
            || [
                heartbeat_timeout_ms,
                command_ttl_ms,
                execution_timeout_ms,
                observation_poll_ms,
                convergence_timeout_ms,
                cleanup_timeout_ms,
                publication_timeout_ms,
                cpu_millis,
                memory_bytes,
                output_max_bytes,
            ]
            .contains(&0)
            || pids == 0
            || command_ttl_ms < execution_timeout_ms
            || convergence_timeout_ms < execution_timeout_ms
        {
            return Err("build Flow configuration is invalid".into());
        }
        Ok(Self {
            builder,
            buildkit_socket_volume_id,
            heartbeat_timeout: chrono_duration(heartbeat_timeout_ms)?,
            command_ttl: chrono_duration(command_ttl_ms)?,
            execution_timeout: chrono_duration(execution_timeout_ms)?,
            observation_poll: chrono_duration(observation_poll_ms)?,
            convergence_timeout: chrono_duration(convergence_timeout_ms)?,
            cleanup_timeout: chrono_duration(cleanup_timeout_ms)?,
            publication_timeout: chrono_duration(publication_timeout_ms)?,
            cpu_millis,
            memory_bytes,
            pids,
            output_max_bytes,
            retry_delay: Duration::from_millis(observation_poll_ms.min(cleanup_timeout_ms)),
        })
    }

    pub(super) fn retry_policy(&self) -> a3s_flow::RetryPolicy {
        a3s_flow::RetryPolicy::fixed(u32::MAX, self.retry_delay)
    }
}

fn validate_builder(builder: &ArtifactRef) -> Result<(), String> {
    builder.validate()?;
    let uri = url::Url::parse(&builder.uri).map_err(|_| "builder artifact URI is invalid")?;
    let expected = format!("@{}", builder.digest);
    if uri.scheme() != "oci"
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
        || !uri.path().ends_with(&expected)
        || !matches!(
            builder.media_type.as_str(),
            "application/vnd.oci.image.manifest.v1+json"
                | "application/vnd.oci.image.index.v1+json"
                | "application/vnd.docker.distribution.manifest.v2+json"
                | "application/vnd.docker.distribution.manifest.list.v2+json"
        )
    {
        return Err("builder must be a credential-free digest-pinned OCI artifact".into());
    }
    Ok(())
}

fn chrono_duration(milliseconds: u64) -> Result<chrono::Duration, String> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| "build Flow duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct BuildFlowRuntimeDependencies {
    pub builds: Arc<dyn IBuildRunRepository>,
    pub sources: Arc<dyn ISourceRevisionRepository>,
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
    pub(super) sources: Arc<dyn ISourceRevisionRepository>,
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
