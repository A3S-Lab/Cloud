use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::application::project_replica_runtime_spec;
use crate::modules::workloads::domain::entities::{
    DeploymentStatus, WorkloadReplicaLifecycle,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use crate::modules::workloads::published::{
    ValidatedWorkloadHealthyRouteTargetCandidate, ValidatedWorkloadHealthyRouteTargetCandidateSet,
    WorkloadHealthyRouteTargetCandidateSet,
};
use async_trait::async_trait;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Exact question accepted by the Workloads owner boundary for healthy route
/// targeting. Callers cannot supply Deployment, replica, or placement evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadHealthyRouteTargetCandidateQuery {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    revision_id: WorkloadRevisionId,
    port_name: String,
}

impl WorkloadHealthyRouteTargetCandidateQuery {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: impl Into<String>,
    ) -> Result<Self, String> {
        let query = Self {
            organization_id,
            project_id,
            environment_id,
            revision_id,
            port_name: port_name.into(),
        };
        query.validate()?;
        Ok(query)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.port_name.is_empty()
        {
            return Err("Workload healthy route-target candidate query is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn revision_id(&self) -> WorkloadRevisionId {
        self.revision_id
    }

    pub fn port_name(&self) -> &str {
        &self.port_name
    }
}

#[async_trait]
pub trait IWorkloadHealthyRouteTargetCandidateQueryPort: Send + Sync {
    async fn find_candidates(
        &self,
        query: WorkloadHealthyRouteTargetCandidateQuery,
    ) -> Result<WorkloadHealthyRouteTargetCandidateSet, RepositoryError>;
}

/// Workloads owner-side policy service. It is the sole component that
/// interprets active revision authority, Deployment/replica binding, and
/// Runtime Unit projection for healthy route targeting.
pub struct WorkloadHealthyRouteTargetCandidateQueryService {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadHealthyRouteTargetCandidateQueryService {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }
}

#[async_trait]
impl IWorkloadHealthyRouteTargetCandidateQueryPort
    for WorkloadHealthyRouteTargetCandidateQueryService
{
    async fn find_candidates(
        &self,
        query: WorkloadHealthyRouteTargetCandidateQuery,
    ) -> Result<WorkloadHealthyRouteTargetCandidateSet, RepositoryError> {
        query
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let revision = self
            .workloads
            .find_revision(query.organization_id(), query.revision_id())
            .await?;
        let workload = self
            .workloads
            .find_workload(query.organization_id(), revision.workload_id)
            .await?;
        if workload.project_id != query.project_id()
            || workload.environment_id != query.environment_id()
        {
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
            .any(|port| port.name == query.port_name())
        {
            return Err(RepositoryError::Conflict(
                "route port is not declared by the workload revision".into(),
            ));
        }
        let deployments = self
            .workloads
            .list_deployments(query.organization_id(), workload.id)
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
        let mut candidates = Vec::with_capacity(deployments.len());
        let mut replica_generations = BTreeSet::new();
        for deployment in deployments {
            let binding = self
                .workloads
                .find_deployment_replica_binding(query.organization_id(), deployment.id)
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
                .find_workload_replica(query.organization_id(), workload.id, binding.replica_id)
                .await?;
            if replica.lifecycle != WorkloadReplicaLifecycle::Desired
                || replica.revision_id != revision.id
                || replica.generation != binding.replica_generation
            {
                continue;
            }
            let member = self
                .workloads
                .find_workload_replica_member(query.organization_id(), replica.id, binding.member_id)
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
            let node_id = deployment.node_id.ok_or_else(|| {
                RepositoryError::Storage("active deployment has no node identity".into())
            })?;
            let command_id = deployment.command_id.ok_or_else(|| {
                RepositoryError::Storage("active deployment has no Runtime command identity".into())
            })?;
            candidates.push((
                binding.replica_id,
                binding.replica_generation,
                deployment.id,
                ValidatedWorkloadHealthyRouteTargetCandidate {
                    node_id,
                    command_id,
                    runtime_unit_id: binding.runtime_unit_id.clone(),
                    runtime_generation: binding.runtime_generation,
                    runtime_spec: spec,
                },
            ));
        }
        candidates.sort_by_key(|(replica_id, replica_generation, deployment_id, _)| {
            (*replica_id, *replica_generation, *deployment_id)
        });
        WorkloadHealthyRouteTargetCandidateSet::from_validated(
            ValidatedWorkloadHealthyRouteTargetCandidateSet {
                organization_id: workload.organization_id,
                workload_id: workload.id,
                revision_id: revision.id,
                port_name: query.port_name().to_owned(),
                candidates: candidates
                    .into_iter()
                    .map(|(_, _, _, candidate)| candidate)
                    .collect(),
            },
        )
        .map_err(RepositoryError::Conflict)
    }
}
