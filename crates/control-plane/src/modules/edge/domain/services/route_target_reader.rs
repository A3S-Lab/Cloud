use crate::modules::edge::domain::{RoutePortName, RouteTarget};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRouteTarget {
    pub workload_id: WorkloadId,
    pub node_id: NodeId,
    pub target: RouteTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRouteTargetSet {
    targets: Vec<ResolvedRouteTarget>,
}

impl ResolvedRouteTargetSet {
    pub fn new(
        expected_member_node_ids: &[NodeId],
        mut targets: Vec<ResolvedRouteTarget>,
    ) -> Result<Self, String> {
        if expected_member_node_ids.is_empty()
            || expected_member_node_ids.len() > 100
            || expected_member_node_ids
                .iter()
                .any(|node_id| node_id.as_uuid().is_nil())
        {
            return Err("route target set requires between one and 100 physical members".into());
        }
        let expected_members = expected_member_node_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if expected_members.len() != expected_member_node_ids.len() {
            return Err("route target set members must be unique".into());
        }
        targets.sort_by_key(|target| target.node_id);
        if targets.len() != expected_members.len()
            || targets
                .windows(2)
                .any(|targets| targets[0].node_id == targets[1].node_id)
            || targets
                .iter()
                .map(|target| target.node_id)
                .collect::<BTreeSet<_>>()
                != expected_members
        {
            return Err(
                "route target set must cover every desired Gateway member exactly once".into(),
            );
        }
        let first = targets
            .first()
            .ok_or_else(|| "route target set must not be empty".to_string())?;
        first.target.validate_for(first.workload_id)?;
        if targets.iter().skip(1).any(|target| {
            target.target.validate_for(target.workload_id).is_err()
                || target.workload_id != first.workload_id
                || target.target.workload_revision_id != first.target.workload_revision_id
                || target.target.port_name != first.target.port_name
        }) {
            return Err(
                "route target set must bind one workload revision and declared port".into(),
            );
        }
        Ok(Self { targets })
    }

    pub fn targets(&self) -> &[ResolvedRouteTarget] {
        &self.targets
    }

    pub fn into_targets(self) -> Vec<ResolvedRouteTarget> {
        self.targets
    }

    pub fn for_member(&self, node_id: NodeId) -> Option<&ResolvedRouteTarget> {
        self.targets
            .binary_search_by_key(&node_id, |target| target.node_id)
            .ok()
            .map(|index| &self.targets[index])
    }
}

#[async_trait]
pub trait IRouteTargetReader: Send + Sync {
    #[allow(clippy::too_many_arguments)]
    async fn resolve_healthy_target(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        revision_id: WorkloadRevisionId,
        port_name: &RoutePortName,
        now: DateTime<Utc>,
    ) -> Result<ResolvedRouteTarget, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
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
        let target = self
            .resolve_healthy_target(
                organization_id,
                project_id,
                environment_id,
                revision_id,
                port_name,
                now,
            )
            .await?;
        ResolvedRouteTargetSet::new(member_node_ids, vec![target])
            .map_err(RepositoryError::Conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{RoutePortName, UpstreamEndpoint};

    fn target(
        workload_id: WorkloadId,
        revision_id: WorkloadRevisionId,
        node_id: NodeId,
        generation: u64,
        port: u16,
        observed_at: DateTime<Utc>,
    ) -> ResolvedRouteTarget {
        ResolvedRouteTarget {
            workload_id,
            node_id,
            target: RouteTarget::new(
                workload_id,
                revision_id,
                format!("workload:{workload_id}:revision:{revision_id}"),
                generation,
                RoutePortName::parse("http").expect("port name"),
                UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}"))
                    .expect("node-local endpoint"),
                observed_at,
            )
            .expect("route target"),
        }
    }

    #[test]
    fn canonical_target_set_covers_every_member() {
        let workload_id = WorkloadId::new();
        let revision_id = WorkloadRevisionId::new();
        let first = NodeId::new();
        let second = NodeId::new();
        let observed_at = Utc::now();
        let target_set = ResolvedRouteTargetSet::new(
            &[first, second],
            vec![
                target(workload_id, revision_id, second, 3, 49153, observed_at),
                target(workload_id, revision_id, first, 3, 49152, observed_at),
            ],
        )
        .expect("target set");

        assert_eq!(target_set.targets().len(), 2);
        assert!(target_set.targets()[0].node_id < target_set.targets()[1].node_id);
        assert_eq!(
            target_set
                .for_member(first)
                .expect("first member")
                .target
                .runtime_generation,
            3
        );
    }

    #[test]
    fn target_set_rejects_partial_duplicate_and_mixed_revision_members() {
        let workload_id = WorkloadId::new();
        let revision_id = WorkloadRevisionId::new();
        let first = NodeId::new();
        let second = NodeId::new();
        let observed_at = Utc::now();
        let first_target = target(workload_id, revision_id, first, 3, 49152, observed_at);

        assert!(ResolvedRouteTargetSet::new(&[first, second], vec![first_target.clone()]).is_err());
        assert!(ResolvedRouteTargetSet::new(
            &[first, second],
            vec![first_target.clone(), first_target.clone()]
        )
        .is_err());
        assert!(ResolvedRouteTargetSet::new(
            &[first, second],
            vec![
                first_target,
                target(
                    workload_id,
                    WorkloadRevisionId::new(),
                    second,
                    4,
                    49153,
                    observed_at,
                ),
            ],
        )
        .is_err());
    }
}
