use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{RoutePortName, RouteTarget};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, Workload, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::infrastructure::runtime_spec::project_runtime_spec;
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
        let spec = project_runtime_spec(&revision).map_err(RepositoryError::Conflict)?;
        Ok(RouteTargetContext {
            workload,
            revision,
            deployments,
            spec,
        })
    }

    async fn resolve_deployment_target(
        &self,
        organization_id: OrganizationId,
        context: &RouteTargetContext,
        deployment: &Deployment,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        let node_id = deployment.node_id.ok_or_else(|| {
            RepositoryError::Storage("active deployment has no node identity".into())
        })?;
        let runtime_command_id = deployment.command_id.ok_or_else(|| {
            RepositoryError::Storage("active deployment has no Runtime command identity".into())
        })?;
        let binding = self
            .workloads
            .find_deployment_replica_binding(organization_id, deployment.id)
            .await?;
        if binding.workload_id != context.workload.id
            || binding.revision_id != context.revision.id
            || binding.node_id != Some(node_id)
            || binding.runtime_unit_id != context.revision.runtime_unit_id()
            || binding.runtime_generation != context.revision.generation
        {
            return Err(RepositoryError::Storage(
                "route target deployment has an inconsistent replica binding".into(),
            ));
        }
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
        if !observation.observation.converges(&context.spec) {
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
            context.spec.unit_id.clone(),
            context.spec.generation,
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
    deployments: Vec<Deployment>,
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
        if context.deployments.len() != 1 {
            return Err(RepositoryError::Conflict(
                "route target must resolve to exactly one active healthy deployment".into(),
            ));
        }
        self.resolve_deployment_target(
            organization_id,
            &context,
            &context.deployments[0],
            port_name,
            now,
        )
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
            let deployments = context
                .deployments
                .iter()
                .filter(|deployment| deployment.node_id == Some(*member_node_id))
                .collect::<Vec<_>>();
            if deployments.len() != 1 {
                return Err(RepositoryError::Conflict(format!(
                    "route target set member {member_node_id} must resolve to exactly one active healthy deployment"
                )));
            }
            targets.push(
                self.resolve_deployment_target(
                    organization_id,
                    &context,
                    deployments[0],
                    port_name,
                    now,
                )
                .await?,
            );
        }
        ResolvedRouteTargetSet::new(member_node_ids, targets).map_err(RepositoryError::Conflict)
    }
}
