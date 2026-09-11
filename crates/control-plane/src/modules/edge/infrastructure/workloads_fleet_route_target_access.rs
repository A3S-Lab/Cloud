use crate::modules::edge::application::IEdgeRuntimeObservationAccess;
use crate::modules::edge::domain::services::{
    IRouteTargetReader, ResolvedRouteTarget, ResolvedRouteTargetSet,
};
use crate::modules::edge::domain::{RoutePortName, RouteTarget};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadRevisionId,
};
use crate::modules::workloads::{
    IWorkloadHealthyRouteTargetCandidateQueryPort, WorkloadHealthyRouteTargetCandidate,
    WorkloadHealthyRouteTargetCandidateQuery, WorkloadHealthyRouteTargetCandidateSet,
};
use a3s_cloud_contracts::RuntimeServiceEndpoint;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;

use super::runtime_http_upstream::gateway_http_upstream;

/// Anti-corruption adapter from Edge healthy route targeting to Workloads
/// candidate facts and Edge Runtime observation access.
pub struct WorkloadsFleetRouteTargetAccessAdapter {
    workloads: Arc<dyn IWorkloadHealthyRouteTargetCandidateQueryPort>,
    observations: Arc<dyn IEdgeRuntimeObservationAccess>,
    observation_max_age: Duration,
}

impl WorkloadsFleetRouteTargetAccessAdapter {
    pub fn new(
        workloads: Arc<dyn IWorkloadHealthyRouteTargetCandidateQueryPort>,
        observations: Arc<dyn IEdgeRuntimeObservationAccess>,
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

    async fn load_candidates(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
    ) -> Result<WorkloadHealthyRouteTargetCandidateSet, RepositoryError> {
        let query = WorkloadHealthyRouteTargetCandidateQuery::new(
            organization_id,
            project_id,
            environment_id,
            revision_id,
            port_name.as_str(),
        )
        .map_err(RepositoryError::Conflict)?;
        self.workloads.find_candidates(query).await
    }

    async fn resolve_candidate(
        &self,
        candidates: &WorkloadHealthyRouteTargetCandidateSet,
        candidate: &WorkloadHealthyRouteTargetCandidate,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        let observation = self
            .observations
            .latest_runtime_observation(
                candidate.node_id(),
                candidate.runtime_unit_id(),
                candidate.runtime_generation(),
            )
            .await?
            .ok_or_else(|| {
                RepositoryError::Conflict("route target has no current Runtime observation".into())
            })?;
        if observation.command_id != Some(candidate.command_id()) {
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
        if !observation.observation.converges(candidate.runtime_spec()) {
            return Err(RepositoryError::Conflict(
                "route target Runtime observation is not healthy at the desired generation".into(),
            ));
        }
        let endpoint =
            RuntimeServiceEndpoint::from_observation(&observation.observation, port_name.as_str())
                .map_err(RepositoryError::Conflict)?;
        let target = RouteTarget::new(
            candidates.workload_id(),
            candidates.revision_id(),
            candidate.runtime_spec().unit_id.clone(),
            candidate.runtime_spec().generation,
            port_name.clone(),
            gateway_http_upstream(&endpoint).map_err(RepositoryError::Conflict)?,
            observation.received_at,
        )
        .map_err(RepositoryError::Conflict)?;
        Ok(ResolvedRouteTarget {
            workload_id: candidates.workload_id(),
            node_id: candidate.node_id(),
            target,
        })
    }
}

#[async_trait]
impl IRouteTargetReader for WorkloadsFleetRouteTargetAccessAdapter {
    async fn resolve_healthy_target(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError> {
        let candidates = self
            .load_candidates(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name,
            )
            .await?;
        if candidates.candidates().len() != 1 {
            return Err(RepositoryError::Conflict(
                "route target must resolve to exactly one active healthy deployment".into(),
            ));
        }
        self.resolve_candidate(&candidates, &candidates.candidates()[0], port_name, now)
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
        let candidates = self
            .load_candidates(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name,
            )
            .await?;
        let mut targets = Vec::with_capacity(member_node_ids.len());
        for member_node_id in member_node_ids {
            let member_candidates = candidates
                .candidates()
                .iter()
                .filter(|candidate| candidate.node_id() == *member_node_id)
                .collect::<Vec<_>>();
            if member_candidates.len() != 1 {
                return Err(RepositoryError::Conflict(format!(
                    "route target set member {member_node_id} must resolve to exactly one active healthy deployment"
                )));
            }
            targets.push(
                self.resolve_candidate(&candidates, member_candidates[0], port_name, now)
                    .await?,
            );
        }
        ResolvedRouteTargetSet::new(member_node_ids, targets).map_err(RepositoryError::Conflict)
    }
}
