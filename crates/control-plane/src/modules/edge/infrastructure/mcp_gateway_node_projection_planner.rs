use crate::modules::edge::domain::repositories::MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY;
use crate::modules::edge::domain::GatewayScope;
use crate::modules::edge::infrastructure::{
    IMcpGatewayProjectionSetPlanner, McpCredentialAuthorityVersion, McpGatewayIngressRoute,
    McpGatewayProjectionAssembler, McpRouteProjectionVersion, PlanMcpGatewayProjectionSet,
    PlannedMcpGatewayProjection, PlannedMcpGatewayProjectionSet,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, McpCredentialId, NodeId, RepositoryError, RouteId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{stream, StreamExt, TryStreamExt};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const LOGICAL_SCOPE_PLANNING_CONCURRENCY: usize = 16;
pub const MAX_MCP_LOGICAL_SCOPES_PER_GATEWAY: usize = 1_000;

#[derive(Debug, Clone)]
pub struct PlanMcpGatewayNodeProjection {
    pub scopes: Vec<GatewayScope>,
    pub gateway_node_id: NodeId,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMcpGatewayNodeProjection {
    gateway_node_id: NodeId,
    observed_at: DateTime<Utc>,
    scope_sets: Vec<PlannedMcpGatewayProjectionSet>,
    route_versions: Vec<McpRouteProjectionVersion>,
    credential_authority_versions: Vec<McpCredentialAuthorityVersion>,
    ingress_routes: Vec<McpGatewayIngressRoute>,
    projection: Option<PlannedMcpGatewayProjection>,
}

impl PlannedMcpGatewayNodeProjection {
    pub fn single(scope_set: PlannedMcpGatewayProjectionSet) -> Result<Self, String> {
        Self::aggregate(vec![scope_set], McpGatewayProjectionAssembler)
    }

    pub fn aggregate(
        mut scope_sets: Vec<PlannedMcpGatewayProjectionSet>,
        assembler: McpGatewayProjectionAssembler,
    ) -> Result<Self, String> {
        if scope_sets.is_empty() || scope_sets.len() > MAX_MCP_LOGICAL_SCOPES_PER_GATEWAY {
            return Err("MCP node projection requires a bounded logical scope set".into());
        }
        scope_sets.sort_by_key(|planned| planned.scope().id);
        if scope_sets
            .windows(2)
            .any(|sets| sets[0].scope().id == sets[1].scope().id)
        {
            return Err("MCP node projection contains a duplicate logical scope".into());
        }

        let gateway_node_id = scope_sets[0].gateway_node_id();
        let observed_at = scope_sets[0].observed_at();
        let organization_id = scope_sets[0].scope().organization_id;
        let mut route_ids = BTreeSet::<RouteId>::new();
        let mut route_versions = Vec::new();
        let mut ingress_routes = Vec::new();
        let mut credential_authority =
            BTreeMap::<McpCredentialId, McpCredentialAuthorityVersion>::new();
        let mut fragments = Vec::new();

        for planned in &scope_sets {
            planned.scope().validate()?;
            if planned.gateway_node_id() != gateway_node_id
                || planned.observed_at() != observed_at
                || planned.scope().organization_id != organization_id
                || planned.scope().updated_at > observed_at
                || !planned.scope().contains_member(gateway_node_id)
                    && (planned.projection().is_some()
                        || !planned.route_versions().is_empty()
                        || !planned.credential_authority_versions().is_empty()
                        || !planned.ingress_routes().is_empty())
            {
                return Err(
                    "MCP node projection contains inconsistent scope observation evidence".into(),
                );
            }
            for version in planned.route_versions() {
                if !route_ids.insert(version.route_id()) {
                    return Err("MCP node projection contains a duplicate route identity".into());
                }
                route_versions.push(version.clone());
            }
            if route_versions.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY {
                return Err("MCP node projection exceeds the complete route bound".into());
            }
            ingress_routes.extend_from_slice(planned.ingress_routes());
            for version in planned.credential_authority_versions() {
                match credential_authority.get(&version.credential_id()) {
                    Some(existing) if existing != version => {
                        return Err(
                            "MCP node projection contains conflicting credential authority".into(),
                        );
                    }
                    Some(_) => {}
                    None => {
                        credential_authority.insert(version.credential_id(), *version);
                    }
                }
            }
            if let Some(projection) = planned.projection() {
                fragments.push(projection.clone());
            }
        }

        route_versions.sort_by_key(McpRouteProjectionVersion::route_id);
        ingress_routes.sort_by_key(McpGatewayIngressRoute::route_id);
        if ingress_routes
            .windows(2)
            .any(|routes| routes[0].route_id() == routes[1].route_id())
        {
            return Err("MCP node projection contains duplicate ingress evidence".into());
        }
        let projection = if fragments.is_empty() {
            None
        } else {
            Some(assembler.assemble(fragments, observed_at)?)
        };
        if projection
            .as_ref()
            .map_or(0, |projection| projection.projection().routes.len())
            != ingress_routes.len()
        {
            return Err("MCP node projection and ingress evidence differ".into());
        }

        Ok(Self {
            gateway_node_id,
            observed_at,
            scope_sets,
            route_versions,
            credential_authority_versions: credential_authority.into_values().collect(),
            ingress_routes,
            projection,
        })
    }

    pub const fn gateway_node_id(&self) -> NodeId {
        self.gateway_node_id
    }

    pub const fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    pub fn scope_sets(&self) -> &[PlannedMcpGatewayProjectionSet] {
        &self.scope_sets
    }

    pub fn primary_scope(&self) -> &GatewayScope {
        self.scope_sets[0].scope()
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

#[async_trait]
pub trait IMcpGatewayNodeProjectionPlanner: Send + Sync {
    async fn plan(
        &self,
        request: PlanMcpGatewayNodeProjection,
    ) -> Result<PlannedMcpGatewayNodeProjection, RepositoryError>;
}

#[derive(Clone)]
pub struct McpGatewayNodeProjectionPlanner {
    scopes: Arc<dyn IMcpGatewayProjectionSetPlanner>,
    assembler: McpGatewayProjectionAssembler,
}

impl McpGatewayNodeProjectionPlanner {
    pub fn new(
        scopes: Arc<dyn IMcpGatewayProjectionSetPlanner>,
        assembler: McpGatewayProjectionAssembler,
    ) -> Self {
        Self { scopes, assembler }
    }
}

#[async_trait]
impl IMcpGatewayNodeProjectionPlanner for McpGatewayNodeProjectionPlanner {
    async fn plan(
        &self,
        mut request: PlanMcpGatewayNodeProjection,
    ) -> Result<PlannedMcpGatewayNodeProjection, RepositoryError> {
        let observed_at = canonical_timestamp(request.observed_at);
        if request.gateway_node_id.as_uuid().is_nil()
            || request.scopes.is_empty()
            || request.scopes.len() > MAX_MCP_LOGICAL_SCOPES_PER_GATEWAY
        {
            return Err(RepositoryError::Conflict(
                "MCP node projection request is invalid".into(),
            ));
        }
        request.scopes.sort_by_key(|scope| scope.id);
        if request
            .scopes
            .windows(2)
            .any(|scopes| scopes[0].id == scopes[1].id)
        {
            return Err(RepositoryError::Storage(
                "MCP node projection scope reader returned duplicates".into(),
            ));
        }
        let planner = Arc::clone(&self.scopes);
        let gateway_node_id = request.gateway_node_id;
        let planned = stream::iter(request.scopes.into_iter().map(|scope| {
            let planner = Arc::clone(&planner);
            async move {
                scope.validate().map_err(RepositoryError::Storage)?;
                if scope.updated_at > observed_at {
                    return Err(RepositoryError::Conflict(
                        "MCP node projection observation predates logical scope state".into(),
                    ));
                }
                if scope.contains_member(gateway_node_id) {
                    planner
                        .plan(PlanMcpGatewayProjectionSet {
                            scope,
                            gateway_node_id,
                            observed_at,
                        })
                        .await
                } else {
                    PlannedMcpGatewayProjectionSet::empty_for_departed_member(
                        scope,
                        gateway_node_id,
                        observed_at,
                    )
                    .map_err(RepositoryError::Conflict)
                }
            }
        }))
        .buffered(LOGICAL_SCOPE_PLANNING_CONCURRENCY)
        .try_collect()
        .await?;
        PlannedMcpGatewayNodeProjection::aggregate(planned, self.assembler)
            .map_err(RepositoryError::Conflict)
    }
}
