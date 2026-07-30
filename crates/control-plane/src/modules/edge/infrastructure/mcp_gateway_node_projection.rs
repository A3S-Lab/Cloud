use crate::modules::edge::domain::repositories::MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY;
use crate::modules::edge::domain::GatewayScope;
use crate::modules::edge::infrastructure::{
    McpGatewayIngressRoute, McpGatewayProjectionAssembler, McpRouteProjectionVersion,
    PlannedMcpGatewayProjection, PlannedMcpGatewayProjectionSet,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, NodeId, OrganizationId, ProjectId, RouteId,
};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpGatewaySnapshotAnchor {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
}

impl McpGatewaySnapshotAnchor {
    pub const fn from_scope(scope: &GatewayScope) -> Self {
        Self {
            organization_id: scope.organization_id,
            project_id: scope.project_id,
            environment_id: scope.environment_id,
            gateway_scope_id: scope.id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
        {
            return Err("MCP Gateway snapshot anchor identity is invalid".into());
        }
        Ok(())
    }

    pub fn matches_scope(&self, scope: &GatewayScope) -> bool {
        self.organization_id == scope.organization_id
            && self.project_id == scope.project_id
            && self.environment_id == scope.environment_id
            && self.gateway_scope_id == scope.id
    }
}

/// Complete MCP desired state for one physical Gateway node. The active
/// logical scopes are canonical and may be empty for a removal snapshot; the
/// immutable anchor remains available for tenant-scoped publication evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMcpGatewayNodeProjection {
    anchor: McpGatewaySnapshotAnchor,
    gateway_node_id: NodeId,
    observed_at: DateTime<Utc>,
    scopes: Vec<GatewayScope>,
    scope_ids: Vec<GatewayScopeId>,
    route_versions: Vec<McpRouteProjectionVersion>,
    ingress_routes: Vec<McpGatewayIngressRoute>,
    projection: Option<PlannedMcpGatewayProjection>,
}

impl PlannedMcpGatewayNodeProjection {
    pub const fn anchor(&self) -> McpGatewaySnapshotAnchor {
        self.anchor
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.anchor.organization_id
    }

    pub const fn gateway_node_id(&self) -> NodeId {
        self.gateway_node_id
    }

    pub const fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    pub fn scopes(&self) -> &[GatewayScope] {
        &self.scopes
    }

    pub fn scope_ids(&self) -> &[GatewayScopeId] {
        &self.scope_ids
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

    pub fn scope(&self, gateway_scope_id: GatewayScopeId) -> Option<&GatewayScope> {
        self.scopes
            .binary_search_by_key(&gateway_scope_id, |scope| scope.id)
            .ok()
            .map(|index| &self.scopes[index])
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct McpGatewayNodeProjectionAssembler {
    projections: McpGatewayProjectionAssembler,
}

impl McpGatewayNodeProjectionAssembler {
    pub fn assemble(
        &self,
        anchor: McpGatewaySnapshotAnchor,
        gateway_node_id: NodeId,
        observed_at: DateTime<Utc>,
        mut sets: Vec<PlannedMcpGatewayProjectionSet>,
    ) -> Result<PlannedMcpGatewayNodeProjection, String> {
        anchor.validate()?;
        if gateway_node_id.as_uuid().is_nil() {
            return Err("MCP Gateway node projection target is invalid".into());
        }
        let observed_at = canonical_timestamp(observed_at);
        sets.sort_by_key(|set| set.scope().id);
        if sets.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY
            || sets
                .windows(2)
                .any(|sets| sets[0].scope().id == sets[1].scope().id)
        {
            return Err("MCP Gateway node projection scope set is invalid".into());
        }
        if let Some(first) = sets.first() {
            if !anchor.matches_scope(first.scope()) {
                return Err(
                    "MCP Gateway node projection anchor is not its first active scope".into(),
                );
            }
        }

        let mut scopes = Vec::with_capacity(sets.len());
        let mut route_versions = BTreeMap::<RouteId, McpRouteProjectionVersion>::new();
        let mut ingress_routes = BTreeMap::<RouteId, McpGatewayIngressRoute>::new();
        let mut ingress_ownership = BTreeSet::new();
        let mut projections = Vec::new();
        for set in sets {
            let (scope, node_id, set_observed_at, versions, ingress, projection) = set.into_parts();
            scope.validate()?;
            if scope.organization_id != anchor.organization_id
                || node_id != gateway_node_id
                || set_observed_at != observed_at
                || !scope.contains_member(gateway_node_id)
                || versions.len() != ingress.len()
                || projection.is_some() != !versions.is_empty()
            {
                return Err(
                    "MCP Gateway node projection contains inconsistent per-scope evidence".into(),
                );
            }
            for version in versions {
                if route_versions.len() == MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY {
                    return Err("MCP Gateway node projection exceeds the route bound".into());
                }
                if version.gateway_scope_id() != scope.id
                    || route_versions.insert(version.route_id(), version).is_some()
                {
                    return Err(
                        "MCP Gateway node projection contains duplicate or cross-scope routes"
                            .into(),
                    );
                }
            }
            for route in ingress {
                if !ingress_ownership.insert((route.hostname().clone(), route.path().to_owned()))
                    || ingress_routes.insert(route.route_id(), route).is_some()
                {
                    return Err(
                        "MCP Gateway node projection contains conflicting ingress ownership".into(),
                    );
                }
            }
            if let Some(projection) = projection {
                projections.push(projection);
            }
            scopes.push(scope);
        }
        if route_versions.len() > MAX_ACTIVE_MCP_ROUTES_PER_GATEWAY
            || route_versions.len() != ingress_routes.len()
            || route_versions.keys().ne(ingress_routes.keys())
        {
            return Err("MCP Gateway node projection route evidence is incomplete".into());
        }
        let projection = if projections.is_empty() {
            None
        } else {
            Some(
                self.projections
                    .assemble_complete(projections, observed_at)?,
            )
        };
        if projection.as_ref().map(|projection| {
            projection
                .projection()
                .routes
                .iter()
                .map(|route| RouteId::from_uuid(route.route_id))
                .ne(route_versions.keys().copied())
        }) == Some(true)
        {
            return Err("MCP Gateway node projection and route evidence differ".into());
        }
        let scope_ids = scopes.iter().map(|scope| scope.id).collect();
        Ok(PlannedMcpGatewayNodeProjection {
            anchor,
            gateway_node_id,
            observed_at,
            scopes,
            scope_ids,
            route_versions: route_versions.into_values().collect(),
            ingress_routes: ingress_routes.into_values().collect(),
            projection,
        })
    }
}
