use crate::modules::edge::domain::repositories::MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY;
use crate::modules::edge::domain::services::IMcpRouteProjectionInputReader;
use crate::modules::edge::domain::{
    DomainClaimState, DomainNamePattern, GatewayScope, RouteHostname,
};
use crate::modules::edge::infrastructure::{
    McpGatewayProjectionAssembler, McpGatewayProjectionPlanner, PlanMcpRouteProjection,
    PlannedMcpGatewayProjection,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, NodeId, RepositoryError, RouteId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::collections::BTreeSet;
use std::sync::Arc;

const ROUTE_PLANNING_CONCURRENCY: usize = 16;

#[derive(Debug, Clone)]
pub struct PlanMcpGatewayProjectionSet {
    pub scope: GatewayScope,
    pub gateway_node_id: NodeId,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewayIngressRoute {
    route_id: RouteId,
    router: String,
    hostname: RouteHostname,
    path: String,
    domain_claim_id: DomainClaimId,
    domain_pattern: DomainNamePattern,
}

impl McpGatewayIngressRoute {
    pub const fn route_id(&self) -> RouteId {
        self.route_id
    }

    pub fn router(&self) -> &str {
        &self.router
    }

    pub const fn hostname(&self) -> &RouteHostname {
        &self.hostname
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub const fn domain_claim_id(&self) -> DomainClaimId {
        self.domain_claim_id
    }

    pub const fn domain_pattern(&self) -> &DomainNamePattern {
        &self.domain_pattern
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRouteProjectionVersion {
    route_id: RouteId,
    policy_revision: u64,
    policy_digest: Sha256Digest,
    workload_id: WorkloadId,
    workload_aggregate_version: u64,
    active_revision_id: WorkloadRevisionId,
    domain_claim_id: DomainClaimId,
    domain_claim_aggregate_version: u64,
}

impl McpRouteProjectionVersion {
    pub const fn route_id(&self) -> RouteId {
        self.route_id
    }

    pub const fn policy_revision(&self) -> u64 {
        self.policy_revision
    }

    pub const fn policy_digest(&self) -> &Sha256Digest {
        &self.policy_digest
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_aggregate_version(&self) -> u64 {
        self.workload_aggregate_version
    }

    pub const fn active_revision_id(&self) -> WorkloadRevisionId {
        self.active_revision_id
    }

    pub const fn domain_claim_id(&self) -> DomainClaimId {
        self.domain_claim_id
    }

    pub const fn domain_claim_aggregate_version(&self) -> u64 {
        self.domain_claim_aggregate_version
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMcpGatewayProjectionSet {
    scope: GatewayScope,
    gateway_node_id: NodeId,
    observed_at: DateTime<Utc>,
    route_versions: Vec<McpRouteProjectionVersion>,
    ingress_routes: Vec<McpGatewayIngressRoute>,
    projection: Option<PlannedMcpGatewayProjection>,
}

impl PlannedMcpGatewayProjectionSet {
    pub const fn scope(&self) -> &GatewayScope {
        &self.scope
    }

    pub const fn gateway_node_id(&self) -> NodeId {
        self.gateway_node_id
    }

    pub const fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    pub fn route_versions(&self) -> &[McpRouteProjectionVersion] {
        &self.route_versions
    }

    pub fn ingress_routes(&self) -> &[McpGatewayIngressRoute] {
        &self.ingress_routes
    }

    pub const fn projection(&self) -> Option<&PlannedMcpGatewayProjection> {
        self.projection.as_ref()
    }

    pub fn into_parts(
        self,
    ) -> (
        GatewayScope,
        NodeId,
        DateTime<Utc>,
        Vec<McpRouteProjectionVersion>,
        Vec<McpGatewayIngressRoute>,
        Option<PlannedMcpGatewayProjection>,
    ) {
        (
            self.scope,
            self.gateway_node_id,
            self.observed_at,
            self.route_versions,
            self.ingress_routes,
            self.projection,
        )
    }
}

/// Plans every active hosted MCP route for one physical Gateway and assembles
/// one complete node-bound projection.
#[derive(Clone)]
pub struct McpGatewayProjectionSetPlanner {
    inputs: Arc<dyn IMcpRouteProjectionInputReader>,
    routes: McpGatewayProjectionPlanner,
    assembler: McpGatewayProjectionAssembler,
}

impl McpGatewayProjectionSetPlanner {
    pub fn new(
        inputs: Arc<dyn IMcpRouteProjectionInputReader>,
        routes: McpGatewayProjectionPlanner,
        assembler: McpGatewayProjectionAssembler,
    ) -> Self {
        Self {
            inputs,
            routes,
            assembler,
        }
    }

    pub async fn plan(
        &self,
        request: PlanMcpGatewayProjectionSet,
    ) -> Result<PlannedMcpGatewayProjectionSet, RepositoryError> {
        request
            .scope
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if !request.scope.contains_member(request.gateway_node_id) {
            return Err(RepositoryError::Conflict(
                "MCP projection receiving Gateway is not a desired scope member".into(),
            ));
        }
        let observed_at = canonical_timestamp(request.observed_at);
        let mut inputs = self
            .inputs
            .list_active_projection_inputs(&request.scope, observed_at)
            .await?;
        if inputs.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY {
            return Err(RepositoryError::Storage(
                "MCP projection input reader exceeded the complete route bound".into(),
            ));
        }
        if inputs.is_empty() {
            return Ok(PlannedMcpGatewayProjectionSet {
                scope: request.scope,
                gateway_node_id: request.gateway_node_id,
                observed_at,
                route_versions: Vec::new(),
                ingress_routes: Vec::new(),
                projection: None,
            });
        }
        inputs.sort_by_key(|input| input.policy.spec().route_id);
        if inputs
            .windows(2)
            .any(|inputs| inputs[0].policy.spec().route_id == inputs[1].policy.spec().route_id)
            || inputs.iter().any(|input| {
                let spec = input.policy.spec();
                spec.organization_id != request.scope.organization_id
                    || spec.project_id != request.scope.project_id
                    || spec.environment_id != request.scope.environment_id
                    || spec.gateway_scope_id != request.scope.id
                    || spec.expires_at <= observed_at
                    || input.workload_aggregate_version == 0
                    || input.domain_claim.id != spec.domain_claim_id
                    || input.domain_claim.organization_id != spec.organization_id
                    || input.domain_claim.project_id != spec.project_id
                    || input.domain_claim.environment_id != spec.environment_id
                    || input.domain_claim.state != DomainClaimState::Verified
                    || input.domain_claim.aggregate_version == 0
                    || input.domain_claim.failure.is_some()
                    || input.domain_claim.verified_at.is_none()
                    || input.domain_claim.revoked_at.is_some()
                    || input.domain_claim.updated_at > observed_at
                    || !input.domain_claim.covers(&spec.hostname)
            })
        {
            return Err(RepositoryError::Storage(
                "MCP projection input reader returned a partial, duplicate, or cross-scope set"
                    .into(),
            ));
        }
        let ingress_ownership = inputs
            .iter()
            .map(|input| {
                (
                    input.policy.spec().hostname.clone(),
                    input.policy.spec().path.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        if ingress_ownership.len() != inputs.len() {
            return Err(RepositoryError::Storage(
                "MCP projection input reader returned duplicate ingress ownership".into(),
            ));
        }

        let route_versions = inputs
            .iter()
            .map(|input| McpRouteProjectionVersion {
                route_id: input.policy.spec().route_id,
                policy_revision: input.policy.policy_revision(),
                policy_digest: input.policy.policy_digest().clone(),
                workload_id: input.policy.spec().workload_id,
                workload_aggregate_version: input.workload_aggregate_version,
                active_revision_id: input.revision.id,
                domain_claim_id: input.domain_claim.id,
                domain_claim_aggregate_version: input.domain_claim.aggregate_version,
            })
            .collect::<Vec<_>>();
        let ingress_routes = inputs
            .iter()
            .map(|input| McpGatewayIngressRoute {
                route_id: input.policy.spec().route_id,
                router: mcp_router_name(input.policy.spec().route_id),
                hostname: input.policy.spec().hostname.clone(),
                path: input.policy.spec().path.clone(),
                domain_claim_id: input.domain_claim.id,
                domain_pattern: input.domain_claim.pattern.clone(),
            })
            .collect::<Vec<_>>();
        let fragments = stream::iter(inputs.into_iter().map(|input| {
            self.routes.plan(PlanMcpRouteProjection {
                policy: input.policy,
                profile_binding: input.profile_binding,
                revision: input.revision,
                scope: request.scope.clone(),
                gateway_node_id: request.gateway_node_id,
                observed_at,
            })
        }))
        .buffered(ROUTE_PLANNING_CONCURRENCY)
        .try_collect()
        .await?;
        let projection = self
            .assembler
            .assemble(fragments, observed_at)
            .map_err(RepositoryError::Conflict)?;
        if projection.projection().routes.len() != ingress_routes.len()
            || projection
                .projection()
                .routes
                .iter()
                .zip(&ingress_routes)
                .any(|(route, ingress)| {
                    route.route_id != ingress.route_id.as_uuid() || route.router != ingress.router
                })
        {
            return Err(RepositoryError::Conflict(
                "MCP ingress bindings differ from their assembled route projection".into(),
            ));
        }
        Ok(PlannedMcpGatewayProjectionSet {
            scope: request.scope,
            gateway_node_id: request.gateway_node_id,
            observed_at,
            route_versions,
            ingress_routes,
            projection: Some(projection),
        })
    }
}

fn mcp_router_name(route_id: RouteId) -> String {
    format!("mcp-route-{}", route_id.as_uuid().simple())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::assets::domain::McpServiceProfileBinding;
    use crate::modules::edge::domain::repositories::IMcpCredentialRepository;
    use crate::modules::edge::domain::services::{
        IRouteTargetReader, ResolvedMcpRouteProjectionInput, ResolvedRouteTarget,
    };
    use crate::modules::edge::domain::{
        DomainClaim, DomainNamePattern, McpCredential, McpRoutePolicy, RoutePortName,
    };
    use crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::{
        fixture, now, target,
    };
    use crate::modules::edge::infrastructure::{
        McpRouteProjectionPlanner, McpRouteTargetProjectionCompiler,
    };
    use crate::modules::edge::InMemoryEdgeRepository;
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, McpCredentialId, OrganizationId, ProjectId, RouteId, WorkloadRevisionId,
    };
    use async_trait::async_trait;
    use chrono::Duration;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const VERIFIER: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    struct FixedInputReader(Vec<ResolvedMcpRouteProjectionInput>);

    #[async_trait]
    impl IMcpRouteProjectionInputReader for FixedInputReader {
        async fn list_active_projection_inputs(
            &self,
            _scope: &GatewayScope,
            _observed_at: DateTime<Utc>,
        ) -> Result<Vec<ResolvedMcpRouteProjectionInput>, RepositoryError> {
            Ok(self.0.clone())
        }
    }

    struct CountingTargetReader {
        target: Option<ResolvedRouteTarget>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IRouteTargetReader for CountingTargetReader {
        async fn resolve_healthy_target(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _environment_id: EnvironmentId,
            _revision_id: WorkloadRevisionId,
            _port_name: &RoutePortName,
            _now: DateTime<Utc>,
        ) -> Result<ResolvedRouteTarget, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.target.clone().ok_or_else(|| {
                RepositoryError::Storage("target resolution must not be called".into())
            })
        }
    }

    fn scope(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
        node_id: NodeId,
    ) -> GatewayScope {
        let spec = fixture.policy.spec();
        GatewayScope::create(
            spec.gateway_scope_id,
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            node_id,
            now(),
        )
        .expect("scope")
    }

    fn input(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> ResolvedMcpRouteProjectionInput {
        let spec = fixture.policy.spec();
        let mut domain_claim = DomainClaim::create(
            spec.domain_claim_id,
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            DomainNamePattern::parse(spec.hostname.as_str()).expect("domain pattern"),
            format!("a3s-cloud-verification={}", spec.domain_claim_id),
            now() - Duration::minutes(1),
        )
        .expect("domain claim");
        domain_claim
            .verify(now() - Duration::seconds(1))
            .expect("verify domain claim");
        ResolvedMcpRouteProjectionInput {
            policy: fixture.policy.clone(),
            domain_claim,
            profile_binding: McpServiceProfileBinding {
                organization_id: spec.organization_id,
                asset_id: spec.asset_id,
                asset_release_id: spec.asset_release_id,
                profile: fixture.profile.clone(),
                created_at: now(),
            },
            revision: fixture.revision.clone(),
            workload_aggregate_version: 2,
        }
    }

    fn credential(
        fixture: &crate::modules::edge::infrastructure::mcp_route_target_projection_compiler::tests::Fixture,
    ) -> McpCredential {
        let spec = fixture.policy.spec();
        McpCredential::issue(
            McpCredentialId::from_uuid(spec.grants[0].credential_id),
            spec.organization_id,
            spec.project_id,
            spec.environment_id,
            "a3s_mcp_abc12345def67890",
            VERIFIER,
            now() + Duration::minutes(30),
            now(),
        )
        .expect("credential")
    }

    fn planner(
        inputs: Vec<ResolvedMcpRouteProjectionInput>,
        targets: Arc<CountingTargetReader>,
        credentials: Arc<dyn IMcpCredentialRepository>,
    ) -> McpGatewayProjectionSetPlanner {
        let route_planner =
            McpRouteProjectionPlanner::new(targets, McpRouteTargetProjectionCompiler);
        McpGatewayProjectionSetPlanner::new(
            Arc::new(FixedInputReader(inputs)),
            McpGatewayProjectionPlanner::new(route_planner, credentials),
            McpGatewayProjectionAssembler,
        )
    }

    #[tokio::test]
    async fn plans_the_complete_active_set_for_one_receiving_gateway() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let targets = Arc::new(CountingTargetReader {
            target: Some(target(&fixture, node_id, 49152)),
            calls: AtomicUsize::new(0),
        });
        let credentials = Arc::new(InMemoryEdgeRepository::new());
        credentials
            .create_mcp_credential(credential(&fixture))
            .await
            .expect("credential");
        let planned = planner(vec![input(&fixture)], targets.clone(), credentials)
            .plan(PlanMcpGatewayProjectionSet {
                scope: scope(&fixture, node_id),
                gateway_node_id: node_id,
                observed_at: now(),
            })
            .await
            .expect("projection set");

        assert_eq!(targets.calls.load(Ordering::SeqCst), 1);
        assert_eq!(planned.gateway_node_id(), node_id);
        assert_eq!(planned.route_versions().len(), 1);
        assert_eq!(
            planned.route_versions()[0].route_id(),
            fixture.policy.spec().route_id
        );
        assert_eq!(planned.route_versions()[0].workload_aggregate_version(), 2);
        assert_eq!(
            planned.route_versions()[0].domain_claim_id(),
            fixture.policy.spec().domain_claim_id
        );
        assert_eq!(
            planned.route_versions()[0].domain_claim_aggregate_version(),
            2
        );
        assert_eq!(planned.ingress_routes().len(), 1);
        assert_eq!(
            planned.ingress_routes()[0].router(),
            format!(
                "mcp-route-{}",
                fixture.policy.spec().route_id.as_uuid().simple()
            )
        );
        assert_eq!(
            planned.ingress_routes()[0].hostname(),
            &fixture.policy.spec().hostname
        );
        let projection = planned.projection().expect("non-empty projection");
        assert_eq!(projection.projection().routes.len(), 1);
        assert_eq!(
            projection.projection().routes[0].targets[0].node_id,
            node_id.as_uuid()
        );
    }

    #[tokio::test]
    async fn represents_an_empty_active_set_without_resolving_runtime() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let targets = Arc::new(CountingTargetReader {
            target: None,
            calls: AtomicUsize::new(0),
        });
        let planned = planner(
            Vec::new(),
            targets.clone(),
            Arc::new(InMemoryEdgeRepository::new()),
        )
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await
        .expect("empty set");

        assert!(planned.projection().is_none());
        assert!(planned.route_versions().is_empty());
        assert!(planned.ingress_routes().is_empty());
        assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_duplicate_inputs_before_resolving_runtime() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let targets = Arc::new(CountingTargetReader {
            target: None,
            calls: AtomicUsize::new(0),
        });
        let duplicate = input(&fixture);
        let result = planner(
            vec![duplicate.clone(), duplicate],
            targets.clone(),
            Arc::new(InMemoryEdgeRepository::new()),
        )
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await;

        assert!(matches!(result, Err(RepositoryError::Storage(_))));
        assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_duplicate_ingress_ownership_before_resolving_runtime() {
        let fixture = fixture();
        let node_id = NodeId::new();
        let targets = Arc::new(CountingTargetReader {
            target: None,
            calls: AtomicUsize::new(0),
        });
        let first = input(&fixture);
        let mut second = first.clone();
        let mut second_spec = second.policy.spec().clone();
        second_spec.route_id = RouteId::new();
        second.policy = McpRoutePolicy::create(second_spec, &second.profile_binding.profile, now())
            .expect("second policy");
        let result = planner(
            vec![first, second],
            targets.clone(),
            Arc::new(InMemoryEdgeRepository::new()),
        )
        .plan(PlanMcpGatewayProjectionSet {
            scope: scope(&fixture, node_id),
            gateway_node_id: node_id,
            observed_at: now(),
        })
        .await;

        assert!(matches!(result, Err(RepositoryError::Storage(_))));
        assert_eq!(targets.calls.load(Ordering::SeqCst), 0);
    }
}
