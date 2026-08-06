use crate::modules::edge::domain::repositories::MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY;
use crate::modules::edge::domain::services::IMcpRouteProjectionInputReader;
use crate::modules::edge::domain::{
    DomainClaimState, DomainNamePattern, GatewayScope, RouteHostname,
};
use crate::modules::edge::infrastructure::{
    McpCredentialAuthorityVersion, McpGatewayProjectionAssembler, McpGatewayProjectionPlanner,
    PlanMcpRouteProjection, PlannedMcpGatewayProjection,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, NodeId, RepositoryError, RouteId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::collections::{BTreeMap, BTreeSet};
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
    domain_pattern: DomainNamePattern,
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

    pub const fn domain_pattern(&self) -> &DomainNamePattern {
        &self.domain_pattern
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMcpGatewayProjectionSet {
    scope: GatewayScope,
    gateway_node_id: NodeId,
    observed_at: DateTime<Utc>,
    route_versions: Vec<McpRouteProjectionVersion>,
    credential_authority_versions: Vec<McpCredentialAuthorityVersion>,
    ingress_routes: Vec<McpGatewayIngressRoute>,
    projection: Option<PlannedMcpGatewayProjection>,
}

impl PlannedMcpGatewayProjectionSet {
    pub fn empty(
        scope: GatewayScope,
        gateway_node_id: NodeId,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        scope.validate()?;
        if !scope.contains_member(gateway_node_id) {
            return Err("empty MCP projection receiving Gateway is not a scope member".into());
        }
        Ok(Self {
            scope,
            gateway_node_id,
            observed_at: canonical_timestamp(observed_at),
            route_versions: Vec::new(),
            credential_authority_versions: Vec::new(),
            ingress_routes: Vec::new(),
            projection: None,
        })
    }

    pub(crate) fn empty_for_departed_member(
        scope: GatewayScope,
        gateway_node_id: NodeId,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        scope.validate()?;
        if scope.contains_member(gateway_node_id) {
            return Err("departed MCP projection Gateway is still a scope member".into());
        }
        Ok(Self {
            scope,
            gateway_node_id,
            observed_at: canonical_timestamp(observed_at),
            route_versions: Vec::new(),
            credential_authority_versions: Vec::new(),
            ingress_routes: Vec::new(),
            projection: None,
        })
    }

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

    pub fn credential_authority_versions(&self) -> &[McpCredentialAuthorityVersion] {
        &self.credential_authority_versions
    }

    pub fn ingress_routes(&self) -> &[McpGatewayIngressRoute] {
        &self.ingress_routes
    }

    pub const fn projection(&self) -> Option<&PlannedMcpGatewayProjection> {
        self.projection.as_ref()
    }
}

/// Plans every active hosted MCP route for one physical Gateway and assembles
/// one complete node-bound projection.
#[async_trait]
pub trait IMcpGatewayProjectionSetPlanner: Send + Sync {
    async fn plan(
        &self,
        request: PlanMcpGatewayProjectionSet,
    ) -> Result<PlannedMcpGatewayProjectionSet, RepositoryError>;
}

#[derive(Clone)]
pub struct McpGatewayProjectionSetPlanner {
    inputs: Arc<dyn IMcpRouteProjectionInputReader>,
    routes: McpGatewayProjectionPlanner,
    assembler: McpGatewayProjectionAssembler,
}

#[async_trait]
impl IMcpGatewayProjectionSetPlanner for McpGatewayProjectionSetPlanner {
    async fn plan(
        &self,
        request: PlanMcpGatewayProjectionSet,
    ) -> Result<PlannedMcpGatewayProjectionSet, RepositoryError> {
        McpGatewayProjectionSetPlanner::plan(self, request).await
    }
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
            return PlannedMcpGatewayProjectionSet::empty(
                request.scope,
                request.gateway_node_id,
                observed_at,
            )
            .map_err(RepositoryError::Conflict);
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
                domain_pattern: input.domain_claim.pattern.clone(),
            })
            .collect::<Vec<_>>();
        let ingress_candidates = inputs
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
        let planned_routes: Vec<_> = stream::iter(inputs.into_iter().map(|input| {
            self.routes.plan_for_reconciliation(PlanMcpRouteProjection {
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
        let mut credential_authority = BTreeMap::new();
        let mut ingress_routes = Vec::new();
        let mut fragments = Vec::new();
        for (planned, ingress) in planned_routes.into_iter().zip(ingress_candidates) {
            let (projection, authority_versions) = planned.into_parts();
            for version in authority_versions {
                match credential_authority.get(&version.credential_id()) {
                    Some(existing) if *existing != version => {
                        return Err(RepositoryError::Storage(
                            "MCP credential authority returned conflicting versions in one observation"
                                .into(),
                        ));
                    }
                    Some(_) => {}
                    None => {
                        credential_authority.insert(version.credential_id(), version);
                    }
                }
            }
            if let Some(projection) = projection {
                ingress_routes.push(ingress);
                fragments.push(projection);
            }
        }
        let projection = if fragments.is_empty() {
            None
        } else {
            Some(
                self.assembler
                    .assemble(fragments, observed_at)
                    .map_err(RepositoryError::Conflict)?,
            )
        };
        if let Some(projection) = &projection {
            if projection.projection().routes.len() != ingress_routes.len()
                || projection
                    .projection()
                    .routes
                    .iter()
                    .zip(&ingress_routes)
                    .any(|(route, ingress)| {
                        route.route_id != ingress.route_id.as_uuid()
                            || route.router != ingress.router
                    })
                || projection.credential_versions().iter().any(|version| {
                    credential_authority
                        .get(&version.credential_id())
                        .is_none_or(|authority| {
                            authority.generation() != version.generation()
                                || authority.aggregate_version() != version.aggregate_version()
                                || !authority.active_at_observed_at()
                        })
                })
            {
                return Err(RepositoryError::Conflict(
                    "MCP ingress or credential authority differs from the assembled projection"
                        .into(),
                ));
            }
        }
        Ok(PlannedMcpGatewayProjectionSet {
            scope: request.scope,
            gateway_node_id: request.gateway_node_id,
            observed_at,
            route_versions,
            credential_authority_versions: credential_authority.into_values().collect(),
            ingress_routes,
            projection,
        })
    }
}

fn mcp_router_name(route_id: RouteId) -> String {
    format!("mcp-route-{}", route_id.as_uuid().simple())
}

#[cfg(test)]
#[path = "mcp_gateway_projection_set_planner_tests.rs"]
mod tests;
