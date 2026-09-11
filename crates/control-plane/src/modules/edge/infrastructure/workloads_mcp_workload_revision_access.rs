use crate::modules::edge::application::{
    EdgeMcpWorkloadRevisionProjectionScope, IEdgeMcpWorkloadRevisionProjectionAccess,
};
use crate::modules::edge::domain::{
    EdgeMcpServiceProfileProjectionBinding, EdgeMcpWorkloadRevisionProjectionBinding,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::entities::{
    Workload, WorkloadDesiredState, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use async_trait::async_trait;
use std::sync::Arc;

/// Sole anti-corruption adapter from Edge MCP revision projection to Workloads.
#[derive(Clone)]
pub struct WorkloadsEdgeMcpWorkloadRevisionProjectionAccessAdapter {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl WorkloadsEdgeMcpWorkloadRevisionProjectionAccessAdapter {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
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
        let workload = self
            .workloads
            .find_workload(scope.organization_id(), scope.workload_id())
            .await
            .map_err(|error| {
                missing_as_storage(
                    error,
                    "active MCP route policy lost its referenced Workload",
                )
            })?;
        let revision_id = workload.active_revision_id.ok_or_else(|| {
            RepositoryError::Conflict(
                "active MCP route policy Workload has no active revision".into(),
            )
        })?;
        let revision = self
            .workloads
            .find_revision(scope.organization_id(), revision_id)
            .await
            .map_err(|error| {
                missing_as_storage(error, "active MCP Workload lost its active revision")
            })?;
        admit_mcp_workload_revision_projection_binding(&workload, &revision, profile)
            .map_err(RepositoryError::Conflict)
    }
}

/// Map a running Workloads revision into the Edge-owned MCP projection fact.
pub(crate) fn admit_mcp_workload_revision_projection_binding(
    workload: &Workload,
    revision: &WorkloadRevision,
    profile: &EdgeMcpServiceProfileProjectionBinding,
) -> Result<EdgeMcpWorkloadRevisionProjectionBinding, String> {
    profile.validate()?;
    if workload.id.as_uuid().is_nil()
        || workload.organization_id.as_uuid().is_nil()
        || workload.project_id.as_uuid().is_nil()
        || workload.environment_id.as_uuid().is_nil()
        || workload.desired_state != WorkloadDesiredState::Running
        || workload.aggregate_version == 0
        || workload.active_revision_id != Some(revision.id)
        || revision.workload_id != workload.id
        || revision.id.as_uuid().is_nil()
        || revision.generation == 0
    {
        return Err("active MCP route does not resolve to its running Workload revision".into());
    }
    let binding = revision
        .mcp_binding()
        .ok_or_else(|| "active MCP route Workload revision is not release-bound".to_owned())?;
    let template = revision.resolved_template()?;
    template.validate()?;
    if !template
        .ports
        .iter()
        .any(|port| port.name == profile.runtime_port())
    {
        return Err("MCP Workload does not declare the bound profile Runtime port".into());
    }
    let health = template
        .health
        .as_ref()
        .ok_or_else(|| "MCP Workload requires the bound profile HTTP health check".to_owned())?;
    if health.port_name != profile.runtime_port() || health.path != profile.health_path() {
        return Err("MCP Workload health check differs from the bound Service profile".into());
    }
    if binding.organization_id() != workload.organization_id
        || binding.organization_id() != profile.organization_id()
        || binding.asset_id() != profile.asset_id()
        || binding.asset_release_id() != profile.asset_release_id()
        || binding.profile_digest() != profile.digest()
    {
        return Err(
            "active MCP route Workload release binding differs from its Service profile".into(),
        );
    }

    let owned = EdgeMcpWorkloadRevisionProjectionBinding::new(
        revision.id,
        revision.workload_id,
        revision.generation,
        revision.created_at,
        binding.organization_id(),
        binding.asset_id(),
        binding.asset_release_id(),
        binding.profile_digest().clone(),
        profile.runtime_port(),
        health.port_name.clone(),
        health.path.clone(),
        workload.aggregate_version,
        workload.updated_at,
    )?;
    owned.matches_profile(profile)?;
    Ok(owned)
}

fn missing_as_storage(error: RepositoryError, message: &str) -> RepositoryError {
    match error {
        RepositoryError::NotFound => RepositoryError::Storage(message.into()),
        error => error,
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
