use crate::modules::edge::application::{
    EdgeMcpServiceProfileScope, EdgeMcpWorkloadRevisionProjectionScope,
    IEdgeMcpServiceProfileAccess, IEdgeMcpWorkloadRevisionProjectionAccess,
};
use crate::modules::edge::domain::repositories::{
    IEdgeRepository, IMcpRoutePolicyRepository, MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY,
};
use crate::modules::edge::domain::services::{
    IMcpRouteProjectionInputReader, ResolvedMcpRouteProjectionInput,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, EdgeMcpServiceProfileProjectionBinding,
    EdgeMcpWorkloadRevisionProjectionBinding, GatewayScope, McpRoutePolicy,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::sync::Arc;

const MATERIALIZATION_CONCURRENCY: usize = 16;

#[derive(Clone)]
pub struct McpRouteProjectionInputReader {
    policies: Arc<dyn IMcpRoutePolicyRepository>,
    edge: Arc<dyn IEdgeRepository>,
    profiles: Arc<dyn IEdgeMcpServiceProfileAccess>,
    revisions: Arc<dyn IEdgeMcpWorkloadRevisionProjectionAccess>,
}

impl McpRouteProjectionInputReader {
    pub fn new(
        policies: Arc<dyn IMcpRoutePolicyRepository>,
        edge: Arc<dyn IEdgeRepository>,
        profiles: Arc<dyn IEdgeMcpServiceProfileAccess>,
        revisions: Arc<dyn IEdgeMcpWorkloadRevisionProjectionAccess>,
    ) -> Self {
        Self {
            policies,
            edge,
            profiles,
            revisions,
        }
    }

    async fn materialize(
        &self,
        scope: &GatewayScope,
        policy: McpRoutePolicy,
        observed_at: DateTime<Utc>,
    ) -> Result<ResolvedMcpRouteProjectionInput, RepositoryError> {
        let spec = policy.spec();
        let domain_claim = self
            .edge
            .find_domain_claim(spec.organization_id, spec.domain_claim_id)
            .await
            .map_err(|error| {
                missing_as_storage(
                    error,
                    "active MCP route policy lost its referenced DomainClaim",
                )
            })?;
        let profile_scope = EdgeMcpServiceProfileScope::new(
            spec.organization_id,
            spec.asset_id,
            spec.asset_release_id,
        )
        .map_err(RepositoryError::Conflict)?;
        let profile_binding = self
            .profiles
            .find_projection_binding(profile_scope)
            .await?
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "active MCP route policy lost its immutable Service profile".into(),
                )
            })?;
        let revision_scope =
            EdgeMcpWorkloadRevisionProjectionScope::new(spec.organization_id, spec.workload_id)
                .map_err(RepositoryError::Conflict)?;
        let revision_binding = self
            .revisions
            .find_active_revision_binding(revision_scope, &profile_binding)
            .await?;
        validate_materialized_input(
            scope,
            &policy,
            &domain_claim,
            &profile_binding,
            &revision_binding,
            observed_at,
        )?;
        Ok(ResolvedMcpRouteProjectionInput {
            policy,
            domain_claim,
            profile_binding,
            revision_binding,
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
    domain_claim: &DomainClaim,
    profile_binding: &EdgeMcpServiceProfileProjectionBinding,
    revision_binding: &EdgeMcpWorkloadRevisionProjectionBinding,
    observed_at: DateTime<Utc>,
) -> Result<(), RepositoryError> {
    profile_binding
        .validate()
        .map_err(RepositoryError::Storage)?;
    revision_binding
        .validate()
        .map_err(RepositoryError::Storage)?;
    revision_binding
        .matches_profile(profile_binding)
        .map_err(RepositoryError::Conflict)?;
    revision_binding
        .matches_policy_spec(policy.spec())
        .map_err(RepositoryError::Conflict)?;
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
        || domain_claim.updated_at > observed_at
        || profile_binding.created_at() > observed_at
        || revision_binding.workload_updated_at() > observed_at
        || revision_binding.created_at() > observed_at
    {
        return Err(RepositoryError::Conflict(
            "MCP projection materialization predates its desired state".into(),
        ));
    }
    if domain_claim.id != spec.domain_claim_id
        || domain_claim.organization_id != spec.organization_id
        || domain_claim.project_id != spec.project_id
        || domain_claim.environment_id != spec.environment_id
        || domain_claim.state != DomainClaimState::Verified
        || domain_claim.aggregate_version == 0
        || domain_claim.failure.is_some()
        || domain_claim.verified_at.is_none()
        || domain_claim.revoked_at.is_some()
        || !domain_claim.covers(&spec.hostname)
    {
        return Err(RepositoryError::Conflict(
            "active MCP route does not have exact verified domain authority".into(),
        ));
    }
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
    use crate::modules::edge::DomainNamePattern;
    use crate::modules::shared_kernel::domain::DomainClaimId;
    use chrono::Duration;

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

    fn domain_claim(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> DomainClaim {
        let spec = fixture.policy.spec();
        let mut claim = DomainClaim::create(
            spec.domain_claim_id,
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            DomainNamePattern::parse(spec.hostname.as_str()).expect("domain pattern"),
            format!("a3s-cloud-verification={}", DomainClaimId::new()),
            now() - Duration::minutes(1),
        )
        .expect("claim");
        claim
            .verify(now() - Duration::seconds(1))
            .expect("verify claim");
        claim
    }

    #[test]
    fn accepts_only_the_exact_running_release_bound_revision() {
        let fixture = fixture();
        validate_materialized_input(
            &scope(&fixture),
            &fixture.policy,
            &domain_claim(&fixture),
            &fixture.profile,
            &fixture.revision,
            now(),
        )
        .expect("materialized input");
    }

    #[test]
    fn rejects_revoked_or_cross_tenant_domain_authority() {
        let fixture = fixture();
        let scope = scope(&fixture);
        let mut revoked = domain_claim(&fixture);
        revoked.revoke("revoked", now()).expect("revoke claim");
        assert!(matches!(
            validate_materialized_input(
                &scope,
                &fixture.policy,
                &revoked,
                &fixture.profile,
                &fixture.revision,
                now(),
            ),
            Err(RepositoryError::Conflict(_))
        ));

        let mut foreign = domain_claim(&fixture);
        foreign.organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
        assert!(matches!(
            validate_materialized_input(
                &scope,
                &fixture.policy,
                &foreign,
                &fixture.profile,
                &fixture.revision,
                now(),
            ),
            Err(RepositoryError::Conflict(_))
        ));
    }

    #[test]
    fn rejects_revision_binding_that_diverges_from_policy_or_profile() {
        let fixture = fixture();
        let diverged = EdgeMcpWorkloadRevisionProjectionBinding::new(
            fixture.revision.revision_id(),
            crate::modules::shared_kernel::domain::WorkloadId::new(),
            fixture.revision.generation(),
            fixture.revision.created_at(),
            fixture.revision.organization_id(),
            fixture.revision.asset_id(),
            fixture.revision.asset_release_id(),
            fixture.revision.profile_digest().clone(),
            fixture.revision.runtime_port(),
            fixture.revision.runtime_port(),
            fixture.revision.health_path(),
            fixture.revision.workload_aggregate_version(),
            fixture.revision.workload_updated_at(),
        )
        .expect("diverged revision");
        assert!(matches!(
            validate_materialized_input(
                &scope(&fixture),
                &fixture.policy,
                &domain_claim(&fixture),
                &fixture.profile,
                &diverged,
                now(),
            ),
            Err(RepositoryError::Conflict(_))
        ));
    }
}
