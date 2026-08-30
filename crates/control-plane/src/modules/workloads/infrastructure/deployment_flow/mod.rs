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

use crate::infrastructure::flow_step_retry_policy;
use crate::modules::fleet::domain::repositories::{
    INodeControlRepository, INodeSchedulingRepository,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, OrganizationId, RepositoryError, ResourceClaimId,
};
use crate::modules::workloads::application::{
    DeploymentRuntimeExecutionAdmissionRequest, IWorkloadRuntimeExecutionAdmissionPort,
    NoWorkloadRuntimeExecutionAdmission,
};
pub use crate::modules::workloads::application::{
    DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION, LEGACY_DEPLOYMENT_WORKFLOW_VERSION,
    PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME, PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION, PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
    RESOURCE_CLAIM_DEPLOYMENT_WORKFLOW_VERSION, STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION,
};
use crate::modules::workloads::domain::entities::ResourceClaimState;
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentRuntimeExecutionBinding, DeploymentStatus, Workload, WorkloadControl,
    WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::{
    IDeploymentFlowWorkloadRepository, IResourceClaimRepository,
};
use crate::modules::workloads::domain::services::{
    IDeploymentRouteUpdater, IOciArtifactResolver, IWorkloadPrestartGate,
    UnrestrictedWorkloadPrestartGate,
};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowContext, WorkflowInvocation,
};
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

    pub(super) fn retry_policy(&self, context: &WorkflowContext<'_>) -> a3s_flow::RetryPolicy {
        flow_step_retry_policy(context, self.retry_delay)
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
    pub(super) runtime_execution_admission: Arc<dyn IWorkloadRuntimeExecutionAdmissionPort>,
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
    runtime_execution_admission: Arc<dyn IWorkloadRuntimeExecutionAdmissionPort>,
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
            runtime_execution_admission: Arc::new(NoWorkloadRuntimeExecutionAdmission),
        }
    }

    pub fn with_prestart_gate(mut self, prestart_gate: Arc<dyn IWorkloadPrestartGate>) -> Self {
        self.prestart_gate = prestart_gate;
        self
    }

    pub fn with_runtime_execution_admission(
        mut self,
        runtime_execution_admission: Arc<dyn IWorkloadRuntimeExecutionAdmissionPort>,
    ) -> Self {
        self.runtime_execution_admission = runtime_execution_admission;
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
            runtime_execution_admission: dependencies.runtime_execution_admission,
            heartbeat_timeout,
            config,
        })
    }
}

pub(crate) fn flow_step_names() -> impl Iterator<Item = &'static str> {
    steps::STEP_NAMES
        .iter()
        .chain(placement_group_workflow::STEP_NAMES)
        .chain(placement_group_workflow_v2::STEP_NAMES)
        .chain(stop_workflow::STEP_NAMES)
        .copied()
}

pub(crate) fn flow_workflow_identities() -> impl Iterator<Item = (&'static str, &'static str)> {
    [
        (DEPLOYMENT_WORKFLOW_NAME, DEPLOYMENT_WORKFLOW_VERSION),
        (
            DEPLOYMENT_WORKFLOW_NAME,
            RESOURCE_CLAIM_DEPLOYMENT_WORKFLOW_VERSION,
        ),
        (
            DEPLOYMENT_WORKFLOW_NAME,
            PREVIOUS_DEPLOYMENT_WORKFLOW_VERSION,
        ),
        (DEPLOYMENT_WORKFLOW_NAME, LEGACY_DEPLOYMENT_WORKFLOW_VERSION),
        (
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
        ),
        (
            PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_NAME,
            PREVIOUS_PLACEMENT_GROUP_DEPLOYMENT_WORKFLOW_VERSION,
        ),
        (STOP_WORKFLOW_NAME, STOP_WORKFLOW_VERSION),
    ]
    .into_iter()
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
        let step_name = invocation.step_name.as_str();
        if steps::STEP_NAMES.contains(&step_name) {
            steps::execute(self, invocation).await
        } else if placement_group_workflow::STEP_NAMES.contains(&step_name) {
            placement_group_workflow::execute(self, invocation).await
        } else if placement_group_workflow_v2::STEP_NAMES.contains(&step_name) {
            placement_group_workflow_v2::execute(self, invocation).await
        } else if stop_workflow::STEP_NAMES.contains(&step_name) {
            stop_workflow::execute(self, invocation).await
        } else {
            Err(FlowError::Runtime(format!(
                "Cloud deployment workflow has no step {step_name:?}"
            )))
        }
    }
}

fn flow_error(context: &str, error: impl std::fmt::Display) -> FlowError {
    FlowError::Runtime(format!("{context}: {error}"))
}

fn resource_claim_id(deployment_id: DeploymentId) -> ResourceClaimId {
    ResourceClaimId::from_uuid(deployment_id.as_uuid())
}

fn validate_deployment_runtime_execution_binding(
    binding: &DeploymentRuntimeExecutionBinding,
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
    control: &WorkloadControl,
) -> a3s_flow::Result<()> {
    let validation = if deployment.status == DeploymentStatus::Resolving {
        binding.validate_admission(deployment, workload, revision, control)
    } else {
        binding.validate_lineage(deployment, workload, revision)
    };
    validation.map_err(|error| {
        flow_error(
            "Deployment Runtime execution binding is inconsistent",
            error,
        )
    })
}

async fn admit_deployment_runtime_execution(
    runtime: &DeploymentFlowRuntime,
    deployment: &Deployment,
    workload: &Workload,
    revision: &WorkloadRevision,
    control: &WorkloadControl,
) -> a3s_flow::Result<Option<DeploymentRuntimeExecutionBinding>> {
    if let Some(existing) = runtime
        .workloads
        .find_deployment_runtime_execution_binding(deployment.organization_id, deployment.id)
        .await
        .map_err(|error| flow_error("could not load Deployment Runtime execution binding", error))?
    {
        validate_deployment_runtime_execution_binding(
            &existing, deployment, workload, revision, control,
        )?;
        return Ok(Some(existing));
    }
    if deployment.status != DeploymentStatus::Resolving {
        return Ok(None);
    }
    let request = DeploymentRuntimeExecutionAdmissionRequest::new(
        workload.organization_id,
        workload.project_id,
        workload.environment_id,
        workload.id,
        revision.id,
        deployment.id,
        control.spec.placement_policy.node_pool_id(),
    )
    .map_err(|error| flow_error("could not construct Runtime execution admission", error))?;
    let admitted = runtime
        .runtime_execution_admission
        .admit(request)
        .await
        .map_err(|error| flow_error("could not admit Deployment Runtime execution", error))?;
    let admitted_at = crate::modules::shared_kernel::domain::canonical_timestamp(
        admitted
            .as_ref()
            .map_or(deployment.updated_at, |value| value.authorized_at())
            .max(deployment.updated_at)
            .max(chrono::Utc::now()),
    );
    let binding = match admitted {
        Some(admitted) => {
            let node_pool_id = admitted.node_pool_id();
            let authorized_at = admitted.authorized_at();
            DeploymentRuntimeExecutionBinding::bind(
                deployment,
                workload,
                revision,
                control,
                node_pool_id,
                admitted.into_execution(),
                authorized_at,
                admitted_at,
            )
        }
        None => DeploymentRuntimeExecutionBinding::admit_unbound(
            deployment,
            workload,
            revision,
            control,
            admitted_at,
        ),
    }
    .map_err(|error| flow_error("could not bind admitted Runtime execution", error))?;
    let write = match runtime
        .workloads
        .bind_deployment_runtime_execution(binding.clone())
        .await
    {
        Ok(write) => write,
        Err(RepositoryError::IdempotencyConflict) => {
            let winner = runtime
                .workloads
                .find_deployment_runtime_execution_binding(
                    deployment.organization_id,
                    deployment.id,
                )
                .await
                .map_err(|error| {
                    flow_error(
                        "could not load concurrent Deployment Runtime execution winner",
                        error,
                    )
                })?
                .ok_or_else(|| {
                    FlowError::Runtime(
                        "Deployment Runtime execution conflicted without a durable winner".into(),
                    )
                })?;
            validate_deployment_runtime_execution_binding(
                &winner, deployment, workload, revision, control,
            )?;
            return Ok(Some(winner));
        }
        Err(error) => {
            return Err(flow_error(
                "could not persist Deployment Runtime execution",
                error,
            ))
        }
    };
    if write.value != binding {
        return Err(FlowError::Runtime(
            "Deployment Runtime execution persistence changed the admitted binding".into(),
        ));
    }
    Ok(Some(write.value))
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
