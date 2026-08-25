use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{RoutePortName, RouteTarget};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::application::project_replica_runtime_spec;
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, DeploymentStatus, Workload, WorkloadReplicaLifecycle,
    WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use a3s_cloud_contracts::RuntimeServiceEndpoint;
use a3s_runtime::contract::RuntimeUnitSpec;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;

use super::runtime_http_upstream::gateway_http_upstream;

pub struct WorkloadRouteTargetReader {
    workloads: Arc<dyn IWorkloadRepository>,
    observations: Arc<dyn INodeControlRepository>,
    observation_max_age: Duration,
}

impl WorkloadRouteTargetReader {
    pub fn new(
        workloads: Arc<dyn IWorkloadRepository>,
        observations: Arc<dyn INodeControlRepository>,
        observation_max_age: Duration,
    ) -> Result<Self, String> {
        if observation_max_age <= Duration::zero() {
            return Err("route target observation maximum age must be positive".into());
        }
        Ok(Self {
            workloads,
            observations,
            observation_max_age,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn load_context(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
    ) -> Result<RouteTargetContext, RepositoryError> {
        let revision = self
            .workloads
            .find_revision(organization_id, revision_id)
            .await?;
        let workload = self
            .workloads
            .find_workload(organization_id, revision.workload_id)
            .await?;
        if workload.project_id != project_id || workload.environment_id != environment_id {
            return Err(RepositoryError::NotFound);
        }
        if workload.active_revision_id != Some(revision.id) {
            return Err(RepositoryError::Conflict(
                "route target must be the workload's active immutable revision".into(),
            ));
        }
        let template = revision
            .resolved_template()
            .map_err(RepositoryError::Conflict)?;
        if !template
            .ports
            .iter()
            .any(|port| port.name == port_name.as_str())
        {
            return Err(RepositoryError::Conflict(
                "route port is not declared by the workload revision".into(),
            ));
        }
        let deployments = self
            .workloads
            .list_deployments(organization_id, workload.id)
            .await?
            .into_iter()
            .filter(|deployment| {
                deployment.revision_id == revision.id
                    && matches!(
                        deployment.status,
                        DeploymentStatus::Retiring | DeploymentStatus::Active
                    )
            })
            .collect::<Vec<_>>();
        let mut targets = Vec::with_capacity(deployments.len());
        let mut replica_generations = BTreeSet::new();
        for deployment in deployments {
            let binding = self
                .workloads
                .find_deployment_replica_binding(organization_id, deployment.id)
                .await?;
            if binding.organization_id != workload.organization_id
                || binding.project_id != workload.project_id
                || binding.environment_id != workload.environment_id
                || binding.workload_id != workload.id
                || binding.revision_id != revision.id
            {
                return Err(RepositoryError::Storage(
                    "route target deployment has an inconsistent replica binding".into(),
                ));
            }
            let replica = self
                .workloads
                .find_workload_replica(organization_id, workload.id, binding.replica_id)
                .await?;
            if replica.lifecycle != WorkloadReplicaLifecycle::Desired
                || replica.revision_id != revision.id
                || replica.generation != binding.replica_generation
            {
                continue;
            }
            let member = self
                .workloads
                .find_workload_replica_member(organization_id, replica.id, binding.member_id)
                .await?;
            let spec = project_replica_runtime_spec(&revision, &replica)
                .map_err(RepositoryError::Storage)?;
            binding
                .validate_against(&deployment, &revision, &replica, &member)
                .map_err(RepositoryError::Storage)?;
            if !replica_generations.insert((replica.id, replica.generation)) {
                return Err(RepositoryError::Storage(
                    "route target replica generation has multiple active deployments".into(),
                ));
            }
            targets.push(RouteDeploymentTarget {
                deployment,
                binding,
                spec,
            });
        }
        targets.sort_by_key(|target| {
            (
                target.binding.replica_id,
                target.binding.replica_generation,
                target.deployment.id,
            )
        });
        Ok(RouteTargetContext {
            workload,
            revision,
            targets,
        })
    }

    async fn resolve_deployment_target(
        &self,
        context: &RouteTargetContext,
        candidate: &RouteDeploymentTarget,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        let deployment = &candidate.deployment;
        let node_id = deployment.node_id.ok_or_else(|| {
            RepositoryError::Storage("active deployment has no node identity".into())
        })?;
        let runtime_command_id = deployment.command_id.ok_or_else(|| {
            RepositoryError::Storage("active deployment has no Runtime command identity".into())
        })?;
        let binding = &candidate.binding;
        let observation = self
            .observations
            .latest_runtime_observation(
                node_id,
                &binding.runtime_unit_id,
                binding.runtime_generation,
            )
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict("route target has no current Runtime observation".into())
            })?;
        if observation.command_id != Some(runtime_command_id) {
            return Err(RepositoryError::Conflict(
                "route target Runtime observation belongs to another command".into(),
            ));
        }
        if observation.received_at > now || now - observation.received_at > self.observation_max_age
        {
            return Err(RepositoryError::Conflict(
                "route target Runtime health observation is stale".into(),
            ));
        }
        if !observation.observation.converges(&candidate.spec) {
            return Err(RepositoryError::Conflict(
                "route target Runtime observation is not healthy at the desired generation".into(),
            ));
        }
        let endpoint =
            RuntimeServiceEndpoint::from_observation(&observation.observation, port_name.as_str())
                .map_err(RepositoryError::Conflict)?;
        let target = RouteTarget::new(
            context.workload.id,
            context.revision.id,
            candidate.spec.unit_id.clone(),
            candidate.spec.generation,
            port_name.clone(),
            gateway_http_upstream(&endpoint).map_err(RepositoryError::Conflict)?,
            observation.received_at,
        )
        .map_err(RepositoryError::Conflict)?;
        Ok(ResolvedRouteTarget {
            workload_id: context.workload.id,
            node_id,
            target,
        })
    }
}

struct RouteTargetContext {
    workload: Workload,
    revision: WorkloadRevision,
    targets: Vec<RouteDeploymentTarget>,
}

struct RouteDeploymentTarget {
    deployment: Deployment,
    binding: DeploymentReplicaBinding,
    spec: RuntimeUnitSpec,
}

#[async_trait]
impl IRouteTargetReader for WorkloadRouteTargetReader {
    async fn resolve_healthy_target(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        let context = self
            .load_context(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name,
            )
            .await?;
        if context.targets.len() != 1 {
            return Err(RepositoryError::Conflict(
                "route target must resolve to exactly one active healthy deployment".into(),
            ));
        }
        self.resolve_deployment_target(&context, &context.targets[0], port_name, now)
            .await
    }

    async fn resolve_healthy_target_set(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        member_node_ids: &[NodeId],
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTargetSet, RepositoryError> {
        let expected_members = member_node_ids.iter().copied().collect::<BTreeSet<_>>();
        if member_node_ids.is_empty()
            || member_node_ids.len() > 100
            || expected_members.len() != member_node_ids.len()
            || member_node_ids
                .iter()
                .any(|node_id| node_id.as_uuid().is_nil())
        {
            return Err(RepositoryError::Conflict(
                "route target set requires one to 100 unique physical members".into(),
            ));
        }
        let context = self
            .load_context(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name,
            )
            .await?;
        let mut targets = Vec::with_capacity(member_node_ids.len());
        for member_node_id in member_node_ids {
            let member_targets = context
                .targets
                .iter()
                .filter(|target| target.deployment.node_id == Some(*member_node_id))
                .collect::<Vec<_>>();
            if member_targets.len() != 1 {
                return Err(RepositoryError::Conflict(format!(
                    "route target set member {member_node_id} must resolve to exactly one active healthy deployment"
                )));
            }
            targets.push(
                self.resolve_deployment_target(&context, member_targets[0], port_name, now)
                    .await?,
            );
        }
        ResolvedRouteTargetSet::new(member_node_ids, targets).map_err(RepositoryError::Conflict)
    }
}
