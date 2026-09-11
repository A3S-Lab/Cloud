use crate::modules::edge::application::{
    EdgeMcpWorkloadRevisionProjectionScope, IEdgeMcpWorkloadRevisionProjectionAccess,
};
use crate::modules::edge::domain::{
    EdgeMcpServiceProfileProjectionBinding, EdgeMcpWorkloadRevisionProjectionBinding,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::{
    IWorkloadMcpActiveRevisionProjectionQueryPort, WorkloadMcpActiveRevisionProjectionQuery,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Sole anti-corruption adapter from Edge MCP revision projection to Workloads.
#[derive(Clone)]
pub struct WorkloadsEdgeMcpWorkloadRevisionProjectionAccessAdapter {
    workloads: Arc<dyn IWorkloadMcpActiveRevisionProjectionQueryPort>,
}

impl WorkloadsEdgeMcpWorkloadRevisionProjectionAccessAdapter {
    pub fn new(workloads: Arc<dyn IWorkloadMcpActiveRevisionProjectionQueryPort>) -> Self {
        Self { workloads }
    }
}

#[async_trait]
impl IEdgeMcpWorkloadRevisionProjectionAccess
    for WorkloadsEdgeMcpWorkloadRevisionProjectionAccessAdapter
{
    async fn find_active_revision_binding(
        &self,
        scope: EdgeMcpWorkloadRevisionProjectionScope,
        profile: &EdgeMcpServiceProfileProjectionBinding,
    ) -> Result<EdgeMcpWorkloadRevisionProjectionBinding, RepositoryError> {
        profile
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let query = WorkloadMcpActiveRevisionProjectionQuery::new(
            scope.organization_id(),
            scope.workload_id(),
            profile.asset_id(),
            profile.asset_release_id(),
            profile.digest().clone(),
            profile.runtime_port(),
            profile.health_path(),
        )
        .map_err(RepositoryError::Conflict)?;
        let projection = self.workloads.find_active_projection(query).await?;
        let owned = EdgeMcpWorkloadRevisionProjectionBinding::new(
            projection.revision_id(),
            projection.workload_id(),
            projection.generation(),
            projection.created_at(),
            projection.organization_id(),
            projection.asset_id(),
            projection.asset_release_id(),
            projection.profile_digest().clone(),
            projection.runtime_port(),
            projection.health_port_name(),
            projection.health_path(),
            projection.workload_aggregate_version(),
            projection.workload_updated_at(),
        )
        .map_err(RepositoryError::Conflict)?;
        owned
            .matches_profile(profile)
            .map_err(RepositoryError::Conflict)?;
        Ok(owned)
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::fixture;

    #[test]
    fn fixture_revision_binding_matches_policy_and_profile() {
        let fixture = fixture();
        fixture
            .revision
            .matches_profile(&fixture.profile)
            .expect("fixture revision matches profile");
        fixture
            .revision
            .matches_policy_spec(fixture.policy.spec())
            .expect("fixture revision matches policy");
        assert_eq!(
            fixture.revision.workload_id(),
            fixture.policy.spec().workload_id
        );
        assert!(fixture.revision.workload_aggregate_version() > 0);
    }
}
