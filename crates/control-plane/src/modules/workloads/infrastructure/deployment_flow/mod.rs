mod legacy_workflow;
mod placement_group_workflow;
mod placement_group_workflow_v2;
mod previous_workflow;
mod steps;
mod stop_workflow;
#[cfg(test)]
mod tests;
mod types;
mod workflow;

use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeSchedulingRepository,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, RepositoryError, ResourceClaimId,
};
pub use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION, LEGACY_DEPLOYMENT_WORKFLOW_VERSION,
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION, PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    RESOURCE_CLAIM_DEPLOYMENT_WORKFLOW_VERSION, STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::ResourceClaimState;
use crate::modules::workloads::domain::repositories::{
    IDeploymentFlowWorkloadRepository, IResourceClaimRepository,
};
use crate::modules::workloads::domain::services::{
    IDeploymentRouteUpdater, IOciArtifactResolver, IWorkloadPrestartGate,
    UnrestrictedWorkloadPrestartGate,
};
use a3s_flow::{FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

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
    pub(super) workloads: Arc<dyn IDeploymentFlowWorkloadRepository>,
    pub(super) resource_claims: Arc<dyn IResourceClaimRepository>,
    pub(super) artifacts: Arc<dyn IOciArtifactResolver>,
    pub(super) nodes: Arc<dyn INodeSchedulingRepository>,
    pub(super) node_control: Arc<dyn INodeControlRepository>,
    pub(super) route_updates: Arc<dyn IDeploymentRouteUpdater>,
    pub(super) prestart_gate: Arc<dyn IWorkloadPrestartGate>,
    pub(super) heartbeat_timeout: chrono::Duration,
    pub(super) config: DeploymentFlowConfig,
}

#[derive(Clone)]
pub struct DeploymentFlowDependencies {
    workloads: Arc<dyn IDeploymentFlowWorkloadRepository>,
    resource_claims: Arc<dyn IResourceClaimRepository>,
    artifacts: Arc<dyn IOciArtifactResolver>,
    nodes: Arc<dyn INodeSchedulingRepository>,
    node_control: Arc<dyn INodeControlRepository>,
    route_updates: Arc<dyn IDeploymentRouteUpdater>,
    prestart_gate: Arc<dyn IWorkloadPrestartGate>,
}

impl DeploymentFlowDependencies {
    pub fn new(
        workloads: Arc<dyn IDeploymentFlowWorkloadRepository>,
        resource_claims: Arc<dyn IResourceClaimRepository>,
        artifacts: Arc<dyn IOciArtifactResolver>,
        nodes: Arc<dyn INodeSchedulingRepository>,
        node_control: Arc<dyn INodeControlRepository>,
        route_updates: Arc<dyn IDeploymentRouteUpdater>,
    ) -> Self {
        Self {
            workloads,
            resource_claims,
            artifacts,
            nodes,
            node_control,
            route_updates,
            prestart_gate: Arc::new(UnrestrictedWorkloadPrestartGate),
        }
    }

    pub fn with_prestart_gate(mut self, prestart_gate: Arc<dyn IWorkloadPrestartGate>) -> Self {
        self.prestart_gate = prestart_gate;
        self
    }
}

impl DeploymentFlowRuntime {
    pub fn new(
        dependencies: DeploymentFlowDependencies,
        heartbeat_timeout: chrono::Duration,
        config: DeploymentFlowConfig,
    ) -> Result<Self, String> {
        if heartbeat_timeout <= chrono::Duration::zero() {
            return Err("deployment scheduler heartbeat timeout must be positive".into());
        }
        Ok(Self {
            workloads: dependencies.workloads,
            resource_claims: dependencies.resource_claims,
            artifacts: dependencies.artifacts,
            nodes: dependencies.nodes,
            node_control: dependencies.node_control,
            route_updates: dependencies.route_updates,
            prestart_gate: dependencies.prestart_gate,
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
            (DEPLOYMENT_WORKFLOW_NAME, RESOURCE_CLAIM_DEPLOYMENT_WORKFLOW_VERSION) => {
                workflow::replay_resource_claims(&self.config, invocation)
            }
            (DEPLOYMENT_WORKFLOW_NAME, PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION) => {
                previous_workflow::replay(&self.config, invocation)
            }
            (DEPLOYMENT_WORKFLOW_NAME, LEGACY_DEPLOYMENT_WORKFLOW_VERSION) => {
                legacy_workflow::replay(&self.config, invocation)
            }
            (
                PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
                PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
            ) => placement_group_workflow_v2::replay(&self.config, invocation),
            (
                PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
                PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
            ) => placement_group_workflow::replay(&self.config, invocation),
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
        if invocation
            .step_name
            .starts_with("placement_group_deployment_v2_")
        {
            placement_group_workflow_v2::execute(self, invocation).await
        } else if invocation
            .step_name
            .starts_with("placement_group_deployment_")
        {
            placement_group_workflow::execute(self, invocation).await
        } else if invocation.step_name.starts_with("stop_workload_") {
            stop_workflow::execute(self, invocation).await
        } else {
            steps::execute(self, invocation).await
        }
    }
}

fn flow_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}

fn resource_claim_id(deployment_id: DeploymentId) -> ResourceClaimId {
    ResourceClaimId::from_uuid(deployment_id.as_uuid())
}

async fn cancel_database_reservation(
    runtime: &DeploymentFlowRuntime,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
    at: chrono::DateTime<chrono::Utc>,
) -> a3s_flow::Result<()> {
    let claim = match runtime
        .resource_claims
        .find(organization_id, resource_claim_id(deployment_id))
        .await
    {
        Ok(claim) => claim,
        Err(RepositoryError::NotFound) => return Ok(()),
        Err(error) => {
            return Err(flow_error(
                "could not load database resource reservation for release",
                error,
            ))
        }
    };
    match claim.state {
        ResourceClaimState::Released => Ok(()),
        ResourceClaimState::ReservedInDb => {
            runtime
                .resource_claims
                .cancel_database_reservation(
                    organization_id,
                    claim.id,
                    claim.aggregate_version,
                    at.max(claim.updated_at),
                )
                .await
                .map_err(|error| {
                    flow_error("could not release database resource reservation", error)
                })?;
            Ok(())
        }
        _ => Err(FlowError::Runtime(
            "issued resource claim requires Agent or trusted fencing release evidence".into(),
        )),
    }
}
