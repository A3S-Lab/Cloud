use crate::modules::edge::domain::services::{IRouteTargetReader, RouteTarget};
use crate::modules::edge::domain::{RoutePortName, UpstreamEndpoint};
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::DeploymentStatus;
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::infrastructure::runtime_spec::project_runtime_spec;
use a3s_cloud_contracts::RuntimeServiceEndpoint;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;

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
    ) -> Result<RouteTarget, RepositoryError> {
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
            .await?;
        let deployment = deployments
            .into_iter()
            .find(|deployment| {
                deployment.revision_id == revision.id
                    && deployment.status == DeploymentStatus::Active
            })
            .ok_or_else(|| {
                RepositoryError::Conflict("route target has no active healthy deployment".into())
            })?;
        let node_id = deployment.node_id.ok_or_else(|| {
            RepositoryError::Storage("active deployment has no node identity".into())
        })?;
        let observation = self
            .observations
            .latest_runtime_observation(
                node_id,
                &revision.runtime_unit_id(),
                revision.generation,
            )
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict("route target has no current Runtime observation".into())
            })?;
        if observation.received_at > now || now - observation.received_at > self.observation_max_age
        {
            return Err(RepositoryError::Conflict(
                "route target Runtime health observation is stale".into(),
            ));
        }
        let spec = project_runtime_spec(&revision).map_err(RepositoryError::Conflict)?;
        if !observation.observation.converges(&spec) {
            return Err(RepositoryError::Conflict(
                "route target Runtime observation is not healthy at the desired generation".into(),
            ));
        }
        let endpoint =
            RuntimeServiceEndpoint::from_observation(&observation.observation, port_name.as_str())
                .map_err(RepositoryError::Conflict)?;
        Ok(RouteTarget {
            workload_id: workload.id,
            workload_revision_id: revision.id,
            node_id,
            upstream: UpstreamEndpoint::parse(endpoint.origin)
                .map_err(RepositoryError::Conflict)?,
        })
    }
}
