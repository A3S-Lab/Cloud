use crate::modules::assets::domain::repositories::IMcpServiceProfileRepository;
use crate::modules::assets::domain::McpServiceProfileBinding;
use crate::modules::edge::domain::repositories::{
    IMcpRoutePolicyRepository, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY,
};
use crate::modules::edge::domain::services::{
    IMcpRouteProjectionInputReader, ResolvedMcpRouteProjectionInput,
};
use crate::modules::edge::domain::{GatewayScope, McpRoutePolicy};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use crate::modules::workloads::domain::entities::{
    Workload, WorkloadDesiredState, WorkloadRevision,
};
use crate::modules::workloads::domain::repositories::IWorkloadRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::sync::Arc;

const MATERIALIZATION_CONCURRENCY: usize = 16;

#[derive(Clone)]
pub struct McpRouteProjectionInputReader {
    policies: Arc<dyn IMcpRoutePolicyRepository>,
    profiles: Arc<dyn IMcpServiceProfileRepository>,
    workloads: Arc<dyn IWorkloadRepository>,
}

impl McpRouteProjectionInputReader {
    pub fn new(
        policies: Arc<dyn IMcpRoutePolicyRepository>,
        profiles: Arc<dyn IMcpServiceProfileRepository>,
        workloads: Arc<dyn IWorkloadRepository>,
    ) -> Self {
        Self {
            policies,
            profiles,
            workloads,
        }
    }

    async fn materialize(
        &self,
        scope: &GatewayScope,
        policy: McpRoutePolicy,
        observed_at: DateTime<Utc>,
    ) -> Result<ResolvedMcpRouteProjectionInput, RepositoryError> {
        let spec = policy.spec();
        let profile_binding = self
            .profiles
            .find_mcp_service_profile(spec.organization_id, spec.asset_id, spec.asset_release_id)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "active MCP route policy lost its immutable Service profile".into(),
                )
            })?;
        let workload = self
            .workloads
            .find_workload(spec.organization_id, spec.workload_id)
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
            .find_revision(spec.organization_id, revision_id)
            .await
            .map_err(|error| {
                missing_as_storage(error, "active MCP Workload lost its active revision")
            })?;
        validate_materialized_input(
            scope,
            &policy,
            &profile_binding,
            &workload,
            &revision,
            observed_at,
        )?;
        Ok(ResolvedMcpRouteProjectionInput {
            policy,
            profile_binding,
            revision,
            workload_aggregate_version: workload.aggregate_version,
        })
    }
}

#[async_trait]
impl IMcpRouteProjectionInputReader for McpRouteProjectionInputReader {
    async fn list_active_projection_inputs(
        &self,
        scope: &GatewayScope,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<ResolvedMcpRouteProjectionInput>, RepositoryError> {
        scope.validate().map_err(RepositoryError::Conflict)?;
        let observed_at = canonical_timestamp(observed_at);
        if observed_at < scope.updated_at {
            return Err(RepositoryError::Conflict(
                "MCP projection observation predates Gateway scope desired state".into(),
            ));
        }
        let policies = self
            .policies
            .list_active_mcp_route_policies_for_gateway(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                scope.id,
                observed_at,
            )
            .await?;
        if policies.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY {
            return Err(RepositoryError::Storage(
                "MCP route policy reader exceeded the complete projection bound".into(),
            ));
        }
        stream::iter(
            policies
                .into_iter()
                .map(|policy| self.materialize(scope, policy, observed_at)),
        )
        .buffered(MATERIALIZATION_CONCURRENCY)
        .try_collect()
        .await
    }
}

fn validate_materialized_input(
    scope: &GatewayScope,
    policy: &McpRoutePolicy,
    profile_binding: &McpServiceProfileBinding,
    workload: &Workload,
    revision: &WorkloadRevision,
    observed_at: DateTime<Utc>,
) -> Result<(), RepositoryError> {
    profile_binding
        .validate()
        .map_err(RepositoryError::Storage)?;
    let observed_at = canonical_timestamp(observed_at);
    let spec = policy.spec();
    if spec.organization_id != scope.organization_id
        || spec.project_id != scope.project_id
        || spec.environment_id != scope.environment_id
        || spec.gateway_scope_id != scope.id
        || spec.expires_at <= observed_at
    {
        return Err(RepositoryError::Storage(
            "active MCP route query returned a policy outside its exact scope or validity".into(),
        ));
    }
    if policy.updated_at() > observed_at
        || profile_binding.created_at > observed_at
        || workload.updated_at > observed_at
        || revision.created_at > observed_at
    {
        return Err(RepositoryError::Conflict(
            "MCP projection materialization predates its desired state".into(),
        ));
    }
    if workload.id != spec.workload_id
        || workload.organization_id != spec.organization_id
        || workload.project_id != spec.project_id
        || workload.environment_id != spec.environment_id
        || workload.desired_state != WorkloadDesiredState::Running
        || workload.aggregate_version == 0
        || workload.active_revision_id != Some(revision.id)
        || revision.workload_id != workload.id
    {
        return Err(RepositoryError::Conflict(
            "active MCP route does not resolve to its running Workload revision".into(),
        ));
    }
    let binding = revision.mcp_binding().ok_or_else(|| {
        RepositoryError::Conflict("active MCP route Workload revision is not release-bound".into())
    })?;
    if profile_binding.organization_id != spec.organization_id
        || profile_binding.asset_id != spec.asset_id
        || profile_binding.asset_release_id != spec.asset_release_id
        || profile_binding.profile.digest() != &spec.profile_digest
        || binding.organization_id() != spec.organization_id
        || binding.asset_id() != spec.asset_id
        || binding.asset_release_id() != spec.asset_release_id
        || binding.profile_digest() != &spec.profile_digest
    {
        return Err(RepositoryError::Conflict(
            "active MCP route, Service profile, and Workload release binding differ".into(),
        ));
    }
    revision
        .resolved_template()
        .map_err(RepositoryError::Conflict)?;
    Ok(())
}

fn missing_as_storage(error: RepositoryError, message: &str) -> RepositoryError {
    match error {
        RepositoryError::NotFound => RepositoryError::Storage(message.into()),
        error => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::{
        fixture, now,
    };

    fn scope(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> GatewayScope {
        let spec = fixture.policy.spec();
        GatewayScope::create(
            spec.gateway_scope_id,
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            crate::modules::shared_kernel::domain::NodeId::new(),
            now(),
        )
        .expect("scope")
    }

    fn profile_binding(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> McpServiceProfileBinding {
        let spec = fixture.policy.spec();
        McpServiceProfileBinding {
            organization_id: spec.organization_id,
            asset_id: spec.asset_id,
            asset_release_id: spec.asset_release_id,
            profile: fixture.profile.clone(),
            created_at: now(),
        }
    }

    fn workload(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> Workload {
        let spec = fixture.policy.spec();
        let mut workload = Workload::create(
            spec.workload_id,
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            crate::modules::shared_kernel::domain::ResourceName::parse("MCP runtime")
                .expect("name"),
            now(),
        );
        workload
            .activate(fixture.revision.id, now())
            .expect("activate revision");
        workload
    }

    #[test]
    fn accepts_only_the_exact_running_release_bound_revision() {
        let fixture = fixture();
        validate_materialized_input(
            &scope(&fixture),
            &fixture.policy,
            &profile_binding(&fixture),
            &workload(&fixture),
            &fixture.revision,
            now(),
        )
        .expect("materialized input");
    }

    #[test]
    fn rejects_stopped_or_different_active_workload_state() {
        let fixture = fixture();
        let scope = scope(&fixture);
        let profile = profile_binding(&fixture);
        let mut stopped = workload(&fixture);
        stopped.request_stop(now()).expect("stop");
        assert!(matches!(
            validate_materialized_input(
                &scope,
                &fixture.policy,
                &profile,
                &stopped,
                &fixture.revision,
                now(),
            ),
            Err(RepositoryError::Conflict(_))
        ));

        let mut different = workload(&fixture);
        different
            .activate(
                crate::modules::shared_kernel::domain::WorkloadRevisionId::new(),
                now(),
            )
            .expect("different revision");
        assert!(matches!(
            validate_materialized_input(
                &scope,
                &fixture.policy,
                &profile,
                &different,
                &fixture.revision,
                now(),
            ),
            Err(RepositoryError::Conflict(_))
        ));
    }
}
