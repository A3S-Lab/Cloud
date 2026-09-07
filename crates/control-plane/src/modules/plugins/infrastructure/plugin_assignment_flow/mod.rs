mod authorize_trust;
mod await_confirmation;
mod enqueue_apply;
mod enqueue_plan;
mod lock;
mod observe;
mod plan_selection;
mod resolve_host;
mod store_plan;
mod types;
mod workflow;

#[cfg(test)]
mod tests;

use crate::infrastructure::flow_step_retry_policy;
use crate::modules::artifacts::application::INodeArtifactStore;
use crate::modules::fleet::domain::repositories::{INodeControlRepository, INodeRepository};
use crate::modules::plugins::domain::repositories::{
    IPluginAssignmentRepository, IPluginPlanProjectionRepository, IPluginRegistryRepository,
};
use crate::modules::plugins::domain::services::{
    IPluginPolicyStore, IPluginRegistryCatalog, IPluginTrustRootStore,
};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub(super) const PLUGIN_ASSIGNMENT_LOCK: &str = "plugin_assignment_lock";
pub(super) const PLUGIN_ASSIGNMENT_RESOLVE_HOST: &str = "plugin_assignment_resolve_host";
pub(super) const PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST: &str = "plugin_assignment_authorize_trust";
pub(super) const PLUGIN_ASSIGNMENT_ENQUEUE_PLAN: &str = "plugin_assignment_enqueue_plan";
pub(super) const PLUGIN_ASSIGNMENT_STORE_PLAN: &str = "plugin_assignment_store_plan";
pub(super) const PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION: &str =
    "plugin_assignment_await_confirmation";
pub(super) const PLUGIN_ASSIGNMENT_ENQUEUE_APPLY: &str = "plugin_assignment_enqueue_apply";
pub(super) const PLUGIN_ASSIGNMENT_OBSERVE: &str = "plugin_assignment_observe";

const STEP_NAMES: &[&str] = &[
    PLUGIN_ASSIGNMENT_LOCK,
    PLUGIN_ASSIGNMENT_RESOLVE_HOST,
    PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST,
    PLUGIN_ASSIGNMENT_ENQUEUE_PLAN,
    PLUGIN_ASSIGNMENT_STORE_PLAN,
    PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION,
    PLUGIN_ASSIGNMENT_ENQUEUE_APPLY,
    PLUGIN_ASSIGNMENT_OBSERVE,
];

#[derive(Debug, Clone, Copy)]
pub struct PluginAssignmentFlowConfigOptions {
    pub observation_poll_ms: u64,
    pub command_ttl_ms: u64,
    pub convergence_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct PluginAssignmentFlowConfig {
    pub observation_poll: chrono::Duration,
    pub command_ttl: chrono::Duration,
    pub convergence_timeout: chrono::Duration,
    retry_delay: Duration,
}

impl PluginAssignmentFlowConfig {
    pub fn new(options: PluginAssignmentFlowConfigOptions) -> Result<Self, String> {
        if [
            options.observation_poll_ms,
            options.command_ttl_ms,
            options.convergence_timeout_ms,
        ]
        .contains(&0)
            || options.observation_poll_ms > options.convergence_timeout_ms
            || options.command_ttl_ms > options.convergence_timeout_ms
        {
            return Err("plugin assignment Flow configuration is invalid".into());
        }
        Ok(Self {
            observation_poll: duration(options.observation_poll_ms)?,
            command_ttl: duration(options.command_ttl_ms)?,
            convergence_timeout: duration(options.convergence_timeout_ms)?,
            retry_delay: Duration::from_millis(options.observation_poll_ms),
        })
    }
}

fn duration(milliseconds: u64) -> Result<chrono::Duration, String> {
    i64::try_from(milliseconds)
        .map(chrono::Duration::milliseconds)
        .map_err(|_| "plugin assignment Flow duration exceeds the supported range".into())
}

#[derive(Clone)]
pub struct PluginAssignmentFlowRuntimeDependencies {
    pub assignments: Arc<dyn IPluginAssignmentRepository>,
    pub registries: Arc<dyn IPluginRegistryRepository>,
    pub nodes: Arc<dyn INodeRepository>,
    pub node_control: Arc<dyn INodeControlRepository>,
    pub trust_roots: Arc<dyn IPluginTrustRootStore>,
    pub policies: Arc<dyn IPluginPolicyStore>,
    pub artifacts: Arc<dyn INodeArtifactStore>,
    pub catalog: Arc<dyn IPluginRegistryCatalog>,
    pub projections: Arc<dyn IPluginPlanProjectionRepository>,
}

#[derive(Clone)]
pub struct PluginAssignmentFlowRuntime {
    pub(super) assignments: Arc<dyn IPluginAssignmentRepository>,
    pub(super) registries: Arc<dyn IPluginRegistryRepository>,
    pub(super) nodes: Arc<dyn INodeRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) trust_roots: Arc<dyn IPluginTrustRootStore>,
    pub(super) policies: Arc<dyn IPluginPolicyStore>,
    pub(super) artifacts: Arc<dyn INodeArtifactStore>,
    pub(super) catalog: Arc<dyn IPluginRegistryCatalog>,
    pub(super) projections: Arc<dyn IPluginPlanProjectionRepository>,
    pub(super) config: PluginAssignmentFlowConfig,
}

impl PluginAssignmentFlowRuntime {
    pub fn new(
        dependencies: PluginAssignmentFlowRuntimeDependencies,
        config: PluginAssignmentFlowConfig,
    ) -> Self {
        Self {
            assignments: dependencies.assignments,
            registries: dependencies.registries,
            nodes: dependencies.nodes,
            node_control: dependencies.node_control,
            trust_roots: dependencies.trust_roots,
            policies: dependencies.policies,
            artifacts: dependencies.artifacts,
            catalog: dependencies.catalog,
            projections: dependencies.projections,
            config,
        }
    }

    pub(super) fn retry_policy(&self, context: &WorkflowContext<'_>) -> a3s_flow::RetryPolicy {
        flow_step_retry_policy(context, self.config.retry_delay)
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    STEP_NAMES.iter().copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once((
        crate::modules::plugins::application::PLUGIN_ASSIGNMENT_WORKFLOW_NAME,
        crate::modules::plugins::application::PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
    ))
}

#[async_trait]
impl FlowRuntime for PluginAssignmentFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        workflow::replay(self, invocation)
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            PLUGIN_ASSIGNMENT_LOCK => encode(lock::lock(self, invocation.input_as()?).await?),
            PLUGIN_ASSIGNMENT_RESOLVE_HOST => {
                encode(resolve_host::resolve_host(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_AUTHORIZE_TRUST => {
                encode(authorize_trust::authorize_trust(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_ENQUEUE_PLAN => {
                encode(enqueue_plan::enqueue_plan(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_STORE_PLAN => {
                encode(store_plan::store_plan(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_AWAIT_CONFIRMATION => {
                encode(await_confirmation::await_confirmation(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_ENQUEUE_APPLY => {
                encode(enqueue_apply::enqueue_apply(self, invocation.input_as()?).await?)
            }
            PLUGIN_ASSIGNMENT_OBSERVE => {
                encode(observe::observe(self, invocation.input_as()?).await?)
            }
            step => Err(FlowError::Runtime(format!(
                "Cloud plugin assignment workflow has no step {step:?}"
            ))),
        }
    }
}

fn encode<T: serde::Serialize>(value: T) -> a3s_flow::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(FlowError::from)
}
