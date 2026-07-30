use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayRouteVersion, GatewayScopeState, Route, RouteState,
};
use crate::modules::edge::infrastructure::{
    McpGatewayIngressRoute, McpGatewayProjectionCompiler, PlannedGatewayNodeDesiredState,
    PlannedMcpGatewayNodeProjection,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, EnvironmentId, GatewayCertificateId, NodeId,
    OrganizationId, ProjectId, RouteId,
};
use a3s_cloud_contracts::{GatewayCertificateRequest, GatewaySnapshot, McpGatewayProjection};
use chrono::{DateTime, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewaySnapshotCompilerConfig {
    pub entrypoint_address: String,
    pub management_address: String,
    pub management_path_prefix: String,
    pub management_auth_token_env: String,
    pub upstream_request_timeout_ms: u64,
    pub certificate_directory: String,
    pub managed_state_file: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewaySnapshotMetadata {
    pub node_id: NodeId,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewaySnapshotRouteInput {
    pub route: Route,
    pub domain_claim: DomainClaim,
}

#[derive(Debug, Clone)]
pub struct CompileMcpGatewaySnapshot {
    pub metadata: GatewaySnapshotMetadata,
    pub physical_scope: GatewayScopeState,
    pub certificate_id: Option<GatewayCertificateId>,
    pub active_routes: Vec<GatewaySnapshotRouteInput>,
    pub mcp: PlannedMcpGatewayNodeProjection,
}

#[derive(Debug, Clone)]
pub struct CompileManagedGatewayRouteSnapshot {
    pub metadata: GatewaySnapshotMetadata,
    pub desired_state: PlannedGatewayNodeDesiredState,
    pub certificate_id: GatewayCertificateId,
    pub snapshot_routes: Vec<Route>,
    pub additional_domain_claims: Vec<DomainClaim>,
}

#[derive(Debug, Clone)]
pub struct CompileManagedGatewayRetainedSnapshot {
    pub metadata: GatewaySnapshotMetadata,
    pub desired_state: PlannedGatewayNodeDesiredState,
    pub certificate_id: Option<GatewayCertificateId>,
    pub reused_certificate_request: Option<GatewayCertificateRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GatewayDomainClaimVersion {
    domain_claim_id: DomainClaimId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    aggregate_version: u64,
}

impl GatewayDomainClaimVersion {
    pub const fn domain_claim_id(self) -> DomainClaimId {
        self.domain_claim_id
    }

    pub const fn aggregate_version(self) -> u64 {
        self.aggregate_version
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(self) -> EnvironmentId {
        self.environment_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledMcpGatewaySnapshot {
    snapshot: GatewaySnapshot,
    desired_state_digest: crate::modules::shared_kernel::domain::Sha256Digest,
    physical_scope: GatewayScopeState,
    ordinary_route_ids: Vec<RouteId>,
    active_route_versions: Vec<GatewayRouteVersion>,
    domain_claim_versions: Vec<GatewayDomainClaimVersion>,
    mcp: PlannedMcpGatewayNodeProjection,
}

impl CompiledMcpGatewaySnapshot {
    pub const fn snapshot(&self) -> &GatewaySnapshot {
        &self.snapshot
    }

    pub const fn desired_state_digest(
        &self,
    ) -> &crate::modules::shared_kernel::domain::Sha256Digest {
        &self.desired_state_digest
    }

    pub const fn physical_scope(&self) -> &GatewayScopeState {
        &self.physical_scope
    }

    pub fn active_route_versions(&self) -> &[GatewayRouteVersion] {
        &self.active_route_versions
    }

    pub fn ordinary_route_ids(&self) -> &[RouteId] {
        &self.ordinary_route_ids
    }

    pub fn domain_claim_versions(&self) -> &[GatewayDomainClaimVersion] {
        &self.domain_claim_versions
    }

    pub const fn mcp(&self) -> &PlannedMcpGatewayNodeProjection {
        &self.mcp
    }
}

struct McpSnapshotContent<'a> {
    ingress_routes: &'a [McpGatewayIngressRoute],
    projection: &'a McpGatewayProjection,
}

impl GatewaySnapshotMetadata {
    pub const fn new(
        node_id: NodeId,
        revision: u64,
        expected_revision: Option<u64>,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            node_id,
            revision,
            expected_revision,
            issued_at,
            expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GatewaySnapshotCompiler {
    config: GatewaySnapshotCompilerConfig,
}

impl GatewaySnapshotCompiler {
    pub fn new(config: GatewaySnapshotCompilerConfig) -> Result<Self, String> {
        let entrypoint = config
            .entrypoint_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway entrypoint address: {error}"))?;
        let management = config
            .management_address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid Gateway management address: {error}"))?;
        if entrypoint.port() == 0
            || management.port() == 0
            || !management.ip().is_loopback()
            || !valid_path_prefix(&config.management_path_prefix)
            || !valid_environment_name(&config.management_auth_token_env)
            || config.upstream_request_timeout_ms == 0
            || config.upstream_request_timeout_ms > 3_600_000
            || !valid_certificate_directory(&config.certificate_directory)
            || !valid_absolute_file(&config.managed_state_file)
        {
            return Err("Gateway snapshot compiler configuration is invalid".into());
        }
        Ok(Self { config })
    }

    /// Composes one complete physical Gateway snapshot from the ordinary
    /// active Route set and the complete hosted MCP candidate. The returned
    /// value retains every read-side version needed by durable publication
    /// compare-and-swap.
    pub fn compile_mcp_reconciliation(
        &self,
        request: CompileMcpGatewaySnapshot,
    ) -> Result<CompiledMcpGatewaySnapshot, String> {
        let CompileMcpGatewaySnapshot {
            metadata,
            physical_scope,
            certificate_id,
            active_routes,
            mcp,
        } = request;
        mcp.anchor().validate()?;
        let issued_at = canonical_timestamp(metadata.issued_at);
        let expires_at = canonical_timestamp(metadata.expires_at);
        if physical_scope.node_id.as_uuid().is_nil()
            || physical_scope.installed_revision.is_some_and(|revision| {
                revision == 0 || revision > physical_scope.last_issued_revision
            })
            || if physical_scope.last_issued_revision == 0 {
                physical_scope.aggregate_version != 0 || physical_scope.installed_revision.is_some()
            } else {
                physical_scope.aggregate_version == 0
            }
            || metadata.issued_at != issued_at
            || metadata.expires_at != expires_at
            || mcp.observed_at() != issued_at
            || metadata.node_id != physical_scope.node_id
            || metadata.node_id != mcp.gateway_node_id()
            || metadata.revision != physical_scope.next_revision()?
            || metadata.expected_revision != physical_scope.installed_revision
        {
            return Err(
                "MCP Gateway snapshot metadata does not match its exact planning observation or physical scope"
                    .into(),
            );
        }

        let mut ownership = BTreeSet::new();
        let mut route_versions = BTreeMap::<RouteId, GatewayRouteVersion>::new();
        let mut claim_authority = BTreeMap::<
            DomainClaimId,
            (
                OrganizationId,
                ProjectId,
                EnvironmentId,
                u64,
                crate::modules::edge::domain::DomainNamePattern,
            ),
        >::new();
        let mut routes = Vec::with_capacity(active_routes.len());
        for input in active_routes {
            validate_active_route_authority(&input, metadata.node_id, issued_at)?;
            if input.route.organization_id != mcp.organization_id() {
                return Err(
                    "complete Gateway snapshot crosses the physical node organization".into(),
                );
            }
            let route = input.route;
            let domain_claim = input.domain_claim;
            if !ownership.insert((
                route.hostname.clone(),
                route.path_prefix.as_str().to_owned(),
            )) {
                return Err(
                    "complete Gateway snapshot contains duplicate ordinary Route ownership".into(),
                );
            }
            let version = GatewayRouteVersion::new(route.id, route.aggregate_version)?;
            if route_versions.insert(route.id, version).is_some() {
                return Err("complete Gateway snapshot contains a duplicate Route identity".into());
            }
            insert_claim_authority(
                &mut claim_authority,
                domain_claim.id,
                domain_claim.organization_id,
                domain_claim.project_id,
                domain_claim.environment_id,
                domain_claim.aggregate_version,
                domain_claim.pattern,
            )?;
            routes.push(route);
        }

        let mcp_route_versions = mcp
            .route_versions()
            .iter()
            .map(|version| (version.route_id(), version))
            .collect::<BTreeMap<_, _>>();
        if mcp_route_versions.len() != mcp.route_versions().len()
            || mcp.ingress_routes().len() != mcp.route_versions().len()
        {
            return Err("MCP Gateway snapshot candidate has incomplete route evidence".into());
        }
        for ingress in mcp.ingress_routes() {
            if routes.iter().any(|route| {
                route.hostname == *ingress.hostname()
                    && ingress.path().starts_with(route.path_prefix.as_str())
            }) {
                return Err(
                    "ordinary Gateway PathPrefix would overlap an exact MCP ingress path".into(),
                );
            }
            if !ownership.insert((ingress.hostname().clone(), ingress.path().to_owned())) {
                return Err(
                    "ordinary and MCP Gateway routes have conflicting ingress ownership".into(),
                );
            }
            let version = mcp_route_versions.get(&ingress.route_id()).ok_or_else(|| {
                "MCP Gateway ingress has no exact route version evidence".to_string()
            })?;
            if version.domain_claim_id() != ingress.domain_claim_id() {
                return Err(
                    "MCP Gateway ingress and route version reference different DomainClaims".into(),
                );
            }
            let scope = mcp.scope(version.gateway_scope_id()).ok_or_else(|| {
                "MCP Gateway route evidence references an inactive logical scope".to_string()
            })?;
            insert_claim_authority(
                &mut claim_authority,
                ingress.domain_claim_id(),
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                version.domain_claim_aggregate_version(),
                ingress.domain_pattern().clone(),
            )?;
        }

        let projection = mcp.projection().map(|planned| planned.projection());
        if projection.is_some() != !mcp.ingress_routes().is_empty() {
            return Err(
                "MCP Gateway snapshot candidate projection and ingress cardinality differ".into(),
            );
        }
        if let Some(projection) = projection {
            if projection.expires_at != expires_at {
                return Err(
                    "MCP policy expiry must exactly match the complete managed snapshot expiry"
                        .into(),
                );
            }
        }
        if ownership.is_empty() != certificate_id.is_none() {
            return Err(
                "complete Gateway snapshot requires one certificate exactly when traffic routes exist"
                    .into(),
            );
        }

        let mut ordinary_route_ids = routes.iter().map(|route| route.id).collect::<Vec<_>>();
        ordinary_route_ids.sort();
        let desired_state_digest =
            desired_state_digest(&self.config, &routes, &claim_authority, &mcp)?;
        let content = projection.map(|projection| McpSnapshotContent {
            ingress_routes: mcp.ingress_routes(),
            projection,
        });
        let snapshot =
            self.compile_snapshot(metadata, certificate_id, &routes, false, None, content)?;
        Ok(CompiledMcpGatewaySnapshot {
            snapshot,
            desired_state_digest,
            physical_scope,
            ordinary_route_ids,
            active_route_versions: route_versions.into_values().collect(),
            domain_claim_versions: claim_authority
                .into_iter()
                .map(
                    |(
                        domain_claim_id,
                        (organization_id, project_id, environment_id, aggregate_version, _),
                    )| GatewayDomainClaimVersion {
                        domain_claim_id,
                        organization_id,
                        project_id,
                        environment_id,
                        aggregate_version,
                    },
                )
                .collect(),
            mcp,
        })
    }

    /// Compiles a complete node snapshot without mutating ordinary Routes.
    /// This is used by rollback and certificate lifecycle flows which must
    /// retain the exact active Route version vector while still refreshing MCP
    /// desired state. A reusable certificate request is accepted only when it
    /// covers the complete ordinary-plus-MCP authority set.
    pub fn compile_managed_retained_snapshot(
        &self,
        request: CompileManagedGatewayRetainedSnapshot,
    ) -> Result<CompiledMcpGatewaySnapshot, String> {
        let CompileManagedGatewayRetainedSnapshot {
            metadata,
            desired_state,
            certificate_id,
            reused_certificate_request,
        } = request;
        if reused_certificate_request
            .as_ref()
            .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
            != reused_certificate_request.as_ref().and(certificate_id)
        {
            return Err(
                "managed retained snapshot certificate reuse identity is inconsistent".into(),
            );
        }
        let (physical_scope, active_routes, mcp) = desired_state.into_parts();
        let mut candidate = self.compile_mcp_reconciliation(CompileMcpGatewaySnapshot {
            metadata,
            physical_scope,
            certificate_id,
            active_routes: active_routes.clone(),
            mcp: mcp.clone(),
        })?;
        if let Some(certificate_request) = reused_certificate_request {
            let routes = active_routes
                .iter()
                .map(|input| input.route.clone())
                .collect::<Vec<_>>();
            let content = mcp.projection().map(|planned| McpSnapshotContent {
                ingress_routes: mcp.ingress_routes(),
                projection: planned.projection(),
            });
            candidate.snapshot = self.compile_snapshot(
                metadata,
                certificate_id,
                &routes,
                false,
                Some(certificate_request),
                content,
            )?;
        }
        Ok(candidate)
    }

    /// Compiles a Route mutation and the currently desired MCP policy into one
    /// complete physical-node snapshot. `desired_state.active_routes` is the
    /// pre-write CAS observation; `snapshot_routes` is the post-mutation
    /// candidate that will be sent to Gateway.
    pub fn compile_managed_route_snapshot(
        &self,
        request: CompileManagedGatewayRouteSnapshot,
    ) -> Result<CompiledMcpGatewaySnapshot, String> {
        let CompileManagedGatewayRouteSnapshot {
            metadata,
            desired_state,
            certificate_id,
            snapshot_routes,
            additional_domain_claims,
        } = request;
        let (physical_scope, active_routes, mcp) = desired_state.into_parts();
        mcp.anchor().validate()?;
        let issued_at = canonical_timestamp(metadata.issued_at);
        let expires_at = canonical_timestamp(metadata.expires_at);
        if physical_scope.node_id.as_uuid().is_nil()
            || physical_scope.installed_revision.is_some_and(|revision| {
                revision == 0 || revision > physical_scope.last_issued_revision
            })
            || if physical_scope.last_issued_revision == 0 {
                physical_scope.aggregate_version != 0 || physical_scope.installed_revision.is_some()
            } else {
                physical_scope.aggregate_version == 0
            }
            || metadata.issued_at != issued_at
            || metadata.expires_at != expires_at
            || mcp.observed_at() != issued_at
            || metadata.node_id != physical_scope.node_id
            || metadata.node_id != mcp.gateway_node_id()
            || metadata.revision != physical_scope.next_revision()?
            || metadata.expected_revision != physical_scope.installed_revision
        {
            return Err(
                "managed Gateway Route snapshot metadata does not match its exact planning observation or physical scope"
                    .into(),
            );
        }

        let mut active_route_versions = BTreeMap::<RouteId, GatewayRouteVersion>::new();
        let mut active_route_authority = BTreeMap::<RouteId, DomainClaim>::new();
        let mut claims = BTreeMap::<DomainClaimId, DomainClaim>::new();
        for input in active_routes {
            validate_active_route_authority(&input, metadata.node_id, issued_at)?;
            if input.route.organization_id != mcp.organization_id() {
                return Err(
                    "complete Gateway snapshot crosses the physical node organization".into(),
                );
            }
            let route_id = input.route.id;
            if active_route_versions
                .insert(
                    route_id,
                    GatewayRouteVersion::new(route_id, input.route.aggregate_version)?,
                )
                .is_some()
                || active_route_authority
                    .insert(route_id, input.domain_claim.clone())
                    .is_some()
            {
                return Err(
                    "managed Gateway Route snapshot contains duplicate active Route evidence"
                        .into(),
                );
            }
            insert_domain_claim(&mut claims, input.domain_claim)?;
        }
        for claim in additional_domain_claims {
            validate_verified_domain_claim(&claim, issued_at)?;
            if claim.organization_id != mcp.organization_id() {
                return Err(
                    "managed Gateway Route snapshot additional DomainClaim crosses the node organization"
                        .into(),
                );
            }
            insert_domain_claim(&mut claims, claim)?;
        }

        let mut snapshot_route_ids = BTreeSet::new();
        let mut ownership = BTreeSet::new();
        for route in &snapshot_routes {
            route.validate_target_binding()?;
            if route.gateway_node_id != metadata.node_id
                || route.organization_id != mcp.organization_id()
                || !matches!(route.state, RouteState::Pending | RouteState::Active)
                || route.aggregate_version == 0
            {
                return Err(
                    "managed Gateway Route snapshot contains an ineligible candidate Route".into(),
                );
            }
            if !snapshot_route_ids.insert(route.id)
                || !ownership.insert((
                    route.hostname.clone(),
                    route.path_prefix.as_str().to_owned(),
                ))
            {
                return Err(
                    "managed Gateway Route snapshot contains duplicate Route ownership".into(),
                );
            }
            let claim_id = route.domain_claim_id.ok_or_else(|| {
                "managed Gateway Route snapshot candidate omitted its DomainClaim".to_string()
            })?;
            let claim = claims.get(&claim_id).ok_or_else(|| {
                "managed Gateway Route snapshot candidate has no exact DomainClaim observation"
                    .to_string()
            })?;
            validate_route_domain_authority(route, claim, metadata.node_id, issued_at)?;
            match active_route_authority.get(&route.id) {
                Some(active_claim)
                    if active_claim.id == claim.id
                        && active_claim.organization_id == claim.organization_id
                        && active_claim.project_id == claim.project_id
                        && active_claim.environment_id == claim.environment_id
                        && active_claim.pattern == claim.pattern => {}
                Some(_) => {
                    return Err(
                        "managed Gateway Route mutation changed active Route domain authority"
                            .into(),
                    )
                }
                None if route.state == RouteState::Pending => {}
                None => {
                    return Err(
                        "managed Gateway Route snapshot introduced an unobserved active Route"
                            .into(),
                    )
                }
            }
        }
        if active_route_versions
            .keys()
            .any(|route_id| !snapshot_route_ids.contains(route_id))
            || !snapshot_routes
                .iter()
                .any(|route| route.state == RouteState::Pending)
        {
            return Err(
                "managed Gateway Route mutation must retain every observed active Route and include a pending candidate"
                    .into(),
            );
        }

        let mut claim_authority = claims
            .iter()
            .map(|(id, claim)| {
                (
                    *id,
                    (
                        claim.organization_id,
                        claim.project_id,
                        claim.environment_id,
                        claim.aggregate_version,
                        claim.pattern.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mcp_route_versions = mcp
            .route_versions()
            .iter()
            .map(|version| (version.route_id(), version))
            .collect::<BTreeMap<_, _>>();
        if mcp_route_versions.len() != mcp.route_versions().len()
            || mcp.ingress_routes().len() != mcp.route_versions().len()
        {
            return Err("MCP Gateway snapshot candidate has incomplete route evidence".into());
        }
        for ingress in mcp.ingress_routes() {
            if snapshot_routes.iter().any(|route| {
                route.hostname == *ingress.hostname()
                    && ingress.path().starts_with(route.path_prefix.as_str())
            }) {
                return Err(
                    "ordinary Gateway PathPrefix would overlap an exact MCP ingress path".into(),
                );
            }
            if !ownership.insert((ingress.hostname().clone(), ingress.path().to_owned())) {
                return Err(
                    "ordinary and MCP Gateway routes have conflicting ingress ownership".into(),
                );
            }
            let version = mcp_route_versions.get(&ingress.route_id()).ok_or_else(|| {
                "MCP Gateway ingress has no exact route version evidence".to_string()
            })?;
            if version.domain_claim_id() != ingress.domain_claim_id() {
                return Err(
                    "MCP Gateway ingress and route version reference different DomainClaims".into(),
                );
            }
            let scope = mcp.scope(version.gateway_scope_id()).ok_or_else(|| {
                "MCP Gateway route evidence references an inactive logical scope".to_string()
            })?;
            insert_claim_authority(
                &mut claim_authority,
                ingress.domain_claim_id(),
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                version.domain_claim_aggregate_version(),
                ingress.domain_pattern().clone(),
            )?;
        }

        let projection = mcp.projection().map(|planned| planned.projection());
        if projection.is_some() != !mcp.ingress_routes().is_empty() {
            return Err(
                "MCP Gateway snapshot candidate projection and ingress cardinality differ".into(),
            );
        }
        if let Some(projection) = projection {
            if projection.expires_at != expires_at {
                return Err(
                    "MCP policy expiry must exactly match the complete managed snapshot expiry"
                        .into(),
                );
            }
        }

        let mut ordinary_route_ids = snapshot_routes
            .iter()
            .map(|route| route.id)
            .collect::<Vec<_>>();
        ordinary_route_ids.sort();
        let desired_state_digest =
            desired_state_digest(&self.config, &snapshot_routes, &claim_authority, &mcp)?;
        let content = projection.map(|projection| McpSnapshotContent {
            ingress_routes: mcp.ingress_routes(),
            projection,
        });
        let snapshot = self.compile_snapshot(
            metadata,
            Some(certificate_id),
            &snapshot_routes,
            true,
            None,
            content,
        )?;
        Ok(CompiledMcpGatewaySnapshot {
            snapshot,
            desired_state_digest,
            physical_scope,
            ordinary_route_ids,
            active_route_versions: active_route_versions.into_values().collect(),
            domain_claim_versions: claim_authority
                .into_iter()
                .map(
                    |(
                        domain_claim_id,
                        (organization_id, project_id, environment_id, aggregate_version, _),
                    )| GatewayDomainClaimVersion {
                        domain_claim_id,
                        organization_id,
                        project_id,
                        environment_id,
                        aggregate_version,
                    },
                )
                .collect(),
            mcp,
        })
    }

    pub fn compile(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: GatewayCertificateId,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        self.compile_snapshot(metadata, Some(certificate_id), routes, true, None, None)
    }

    pub fn compile_certificate_convergence(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: Option<GatewayCertificateId>,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        if routes.is_empty() != certificate_id.is_none() {
            return Err(
                "Gateway certificate convergence requires one certificate for non-empty routes"
                    .into(),
            );
        }
        self.compile_snapshot(metadata, certificate_id, routes, false, None, None)
    }

    pub fn compile_certificate_reuse(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_request: GatewayCertificateRequest,
        routes: &[Route],
    ) -> Result<GatewaySnapshot, String> {
        let certificate_id = GatewayCertificateId::from_uuid(certificate_request.certificate_id);
        self.compile_snapshot(
            metadata,
            Some(certificate_id),
            routes,
            false,
            Some(certificate_request),
            None,
        )
    }

    pub fn compile_validity_renewal(
        &self,
        metadata: GatewaySnapshotMetadata,
        current_acl: &str,
    ) -> Result<GatewaySnapshot, String> {
        GatewaySnapshot::new_with_certificate(
            metadata.node_id.as_uuid(),
            metadata.revision,
            metadata.expected_revision,
            metadata.issued_at,
            metadata.expires_at,
            current_acl.to_owned(),
            None,
        )
    }

    fn compile_snapshot(
        &self,
        metadata: GatewaySnapshotMetadata,
        certificate_id: Option<GatewayCertificateId>,
        routes: &[Route],
        require_pending_route: bool,
        certificate_request_override: Option<GatewayCertificateRequest>,
        mcp: Option<McpSnapshotContent<'_>>,
    ) -> Result<GatewaySnapshot, String> {
        let mut routes = routes.iter().collect::<Vec<_>>();
        routes.sort_by(|left, right| {
            (left.hostname.as_str(), left.path_prefix.as_str(), left.id).cmp(&(
                right.hostname.as_str(),
                right.path_prefix.as_str(),
                right.id,
            ))
        });
        let mut ownership = BTreeSet::new();
        let mut dns_names = BTreeSet::new();
        let mut pending_routes = 0_usize;
        for route in &routes {
            route.validate_target_binding()?;
            if route.gateway_node_id != metadata.node_id {
                return Err("complete Gateway snapshot contains a route from another scope".into());
            }
            let state_is_eligible = if require_pending_route {
                matches!(route.state, RouteState::Pending | RouteState::Active)
            } else {
                route.state == RouteState::Active
            };
            if !state_is_eligible {
                return Err("complete Gateway snapshot contains an ineligible route state".into());
            }
            if !ownership.insert((route.hostname.as_str(), route.path_prefix.as_str())) {
                return Err("Gateway route ownership is not unique within the scope".into());
            }
            let Some(pattern) = route.domain_pattern.as_ref() else {
                return Err(
                    "complete Gateway snapshot contains a route without domain proof".into(),
                );
            };
            if route.domain_claim_id.is_none() || route.gateway_certificate_id.is_none() {
                return Err("complete Gateway snapshot contains incomplete TLS ownership".into());
            }
            if !pattern.covers(&route.hostname) {
                return Err("Gateway route hostname is outside its verified domain pattern".into());
            }
            dns_names.insert(pattern.as_str().to_owned());
            if route.state == RouteState::Pending {
                pending_routes += 1;
                if route.gateway_certificate_id != certificate_id {
                    return Err(
                        "pending Gateway route does not reference the snapshot certificate".into(),
                    );
                }
            }
        }
        if let Some(mcp) = &mcp {
            for ingress in mcp.ingress_routes {
                dns_names.insert(ingress.domain_pattern().as_str().to_owned());
            }
        }
        if require_pending_route && pending_routes == 0 {
            return Err("complete Gateway publication must contain a pending route".into());
        }

        let certificate_request = match (certificate_id, certificate_request_override) {
            (Some(certificate_id), Some(request)) => {
                request.validate()?;
                let expected = managed_certificate_request(
                    &self.config.certificate_directory,
                    certificate_id,
                    request.dns_names.clone(),
                )?;
                let request_names = request
                    .dns_names
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>();
                if request != expected
                    || dns_names
                        .iter()
                        .any(|dns_name| !request_names.contains(dns_name.as_str()))
                {
                    return Err(
                        "reused Gateway certificate does not cover the complete snapshot".into(),
                    );
                }
                Some(request)
            }
            (Some(certificate_id), None) => Some(managed_certificate_request(
                &self.config.certificate_directory,
                certificate_id,
                dns_names.into_iter().collect(),
            )?),
            (None, None) => None,
            (None, Some(_)) => {
                return Err(
                    "Gateway certificate reuse requires an exact certificate identity".into(),
                )
            }
        };
        let mut acl = format!(
            "# a3s-cloud complete Gateway snapshot {revision}\n\
             mode {{ kind = \"cloud-managed\" }}\n\n\
             managed {{\n  gateway_id = {}\n  state_file = {}\n}}\n\n",
            acl_string(&metadata.node_id.to_string()),
            acl_string(&self.config.managed_state_file),
            revision = metadata.revision,
        );
        if let Some(certificate_request) = &certificate_request {
            acl.push_str(&format!(
                "entrypoints \"a3s-cloud-https\" {{\n  address = {}\n  tls {{\n    cert_file = {}\n    key_file = {}\n    min_version = \"1.2\"\n  }}\n}}\n\n",
                acl_string(&self.config.entrypoint_address),
                acl_string(&certificate_request.certificate_file),
                acl_string(&certificate_request.private_key_file),
            ));
        }
        for route in &routes {
            let name = format!("route-{}", route.id.as_uuid().simple());
            acl.push_str(&format!(
                "routers \"{name}\" {{\n  rule = {}\n  service = \"{name}\"\n  entrypoints = [\"a3s-cloud-https\"]\n}}\n\n# target revision={} unit={} generation={}\nservices \"{name}\" {{\n  load_balancer {{\n    strategy = \"round-robin\"\n    request_timeout = {}\n    servers = [{{ url = {} }}]\n  }}\n}}\n\n",
                acl_string(&format!(
                    "Host(`{}`) && PathPrefix(`{}`)",
                    route.hostname.as_str(),
                    route.path_prefix.as_str()
                )),
                route.target.workload_revision_id,
                route.target.runtime_unit_id,
                route.target.runtime_generation,
                acl_string(&duration(self.config.upstream_request_timeout_ms)),
                acl_string(route.target.upstream.as_str()),
            ));
        }
        if let Some(mcp) = mcp {
            append_mcp_snapshot_acl(&mut acl, &mcp, metadata.issued_at)?;
        }
        acl.push_str(&format!(
            "management {{\n  enabled = true\n  address = {}\n  path_prefix = {}\n  auth_token_env = {}\n  allowed_ips = [\"127.0.0.1\", \"::1\"]\n}}\n",
            acl_string(&self.config.management_address),
            acl_string(&self.config.management_path_prefix),
            acl_string(&self.config.management_auth_token_env),
        ));
        GatewaySnapshot::new_with_certificate(
            metadata.node_id.as_uuid(),
            metadata.revision,
            metadata.expected_revision,
            metadata.issued_at,
            metadata.expires_at,
            acl,
            certificate_request,
        )
    }
}

fn desired_state_digest(
    config: &GatewaySnapshotCompilerConfig,
    routes: &[Route],
    claims: &BTreeMap<
        DomainClaimId,
        (
            OrganizationId,
            ProjectId,
            EnvironmentId,
            u64,
            crate::modules::edge::domain::DomainNamePattern,
        ),
    >,
    mcp: &PlannedMcpGatewayNodeProjection,
) -> Result<crate::modules::shared_kernel::domain::Sha256Digest, String> {
    let mut routes = routes.iter().collect::<Vec<_>>();
    routes.sort_by_key(|route| route.id);
    let ordinary_routes = routes
        .into_iter()
        .map(|route| {
            json!({
                "id": route.id,
                "organization_id": route.organization_id,
                "project_id": route.project_id,
                "environment_id": route.environment_id,
                "gateway_scope_id": route.gateway_scope_id,
                "gateway_node_id": route.gateway_node_id,
                "hostname": route.hostname,
                "path_prefix": route.path_prefix,
                "domain_claim_id": route.domain_claim_id,
                "domain_pattern": route.domain_pattern,
                "workload_id": route.workload_id,
                "workload_revision_id": route.target.workload_revision_id,
                "runtime_unit_id": route.target.runtime_unit_id,
                "runtime_generation": route.target.runtime_generation,
                "port_name": route.target.port_name,
                "upstream": route.target.upstream,
            })
        })
        .collect::<Vec<_>>();
    let claim_versions = claims
        .iter()
        .map(
            |(
                claim_id,
                (organization_id, project_id, environment_id, aggregate_version, pattern),
            )| {
                json!({
                    "domain_claim_id": claim_id,
                    "organization_id": organization_id,
                    "project_id": project_id,
                    "environment_id": environment_id,
                    "aggregate_version": aggregate_version,
                    "pattern": pattern,
                })
            },
        )
        .collect::<Vec<_>>();
    let mut route_versions = mcp.route_versions().iter().collect::<Vec<_>>();
    route_versions.sort_by_key(|version| version.route_id());
    let route_versions = route_versions
        .into_iter()
        .map(|version| {
            json!({
                "route_id": version.route_id(),
                "gateway_scope_id": version.gateway_scope_id(),
                "policy_revision": version.policy_revision(),
                "policy_digest": version.policy_digest(),
                "workload_id": version.workload_id(),
                "workload_aggregate_version": version.workload_aggregate_version(),
                "active_revision_id": version.active_revision_id(),
                "domain_claim_id": version.domain_claim_id(),
                "domain_claim_aggregate_version": version.domain_claim_aggregate_version(),
            })
        })
        .collect::<Vec<_>>();
    let mut ingress_routes = mcp.ingress_routes().iter().collect::<Vec<_>>();
    ingress_routes.sort_by_key(|ingress| ingress.route_id());
    let ingress_routes = ingress_routes
        .into_iter()
        .map(|ingress| {
            json!({
                "route_id": ingress.route_id(),
                "router": ingress.router(),
                "hostname": ingress.hostname(),
                "path": ingress.path(),
                "domain_claim_id": ingress.domain_claim_id(),
                "domain_pattern": ingress.domain_pattern(),
            })
        })
        .collect::<Vec<_>>();
    let mut credential_versions = mcp
        .projection()
        .map(|projection| projection.credential_versions().iter().collect::<Vec<_>>())
        .unwrap_or_default();
    credential_versions.sort_by_key(|version| version.credential_id());
    let credential_versions = credential_versions
        .into_iter()
        .map(|version| {
            json!({
                "credential_id": version.credential_id(),
                "generation": version.generation(),
                "aggregate_version": version.aggregate_version(),
            })
        })
        .collect::<Vec<_>>();
    let scopes = mcp
        .scopes()
        .iter()
        .map(|scope| {
            json!({
                "id": scope.id,
                "organization_id": scope.organization_id,
                "project_id": scope.project_id,
                "environment_id": scope.environment_id,
                "primary_node_id": scope.node_id,
                "member_node_ids": scope.member_node_ids,
                "membership_generation": scope.membership_generation,
                "min_ready": scope.rollout_policy.min_ready,
                "max_unavailable": scope.rollout_policy.max_unavailable,
                "aggregate_version": scope.aggregate_version,
            })
        })
        .collect::<Vec<_>>();
    let canonical = serde_json::to_vec(&json!({
        "schema": "a3s.cloud.mcp-gateway-desired-state.v2",
        "compiler": {
            "entrypoint_address": config.entrypoint_address,
            "management_address": config.management_address,
            "management_path_prefix": config.management_path_prefix,
            "management_auth_token_env": config.management_auth_token_env,
            "upstream_request_timeout_ms": config.upstream_request_timeout_ms,
            "certificate_directory": config.certificate_directory,
            "managed_state_file": config.managed_state_file,
        },
        "organization_id": mcp.organization_id(),
        "gateway_node_id": mcp.gateway_node_id(),
        "scopes": scopes,
        "ordinary_routes": ordinary_routes,
        "domain_claim_versions": claim_versions,
        "mcp_route_versions": route_versions,
        "mcp_ingress_routes": ingress_routes,
        "mcp_projection": mcp.projection().map(|projection| projection.projection()),
        "credential_versions": credential_versions,
    }))
    .map_err(|error| format!("could not encode MCP Gateway desired state: {error}"))?;
    crate::modules::shared_kernel::domain::Sha256Digest::parse(format!(
        "sha256:{:x}",
        Sha256::digest(canonical)
    ))
}

fn validate_active_route_authority(
    input: &GatewaySnapshotRouteInput,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> Result<(), String> {
    let route = &input.route;
    let claim = &input.domain_claim;
    route.validate_target_binding()?;
    if route.gateway_node_id != node_id
        || route.state != RouteState::Active
        || route.aggregate_version == 0
        || route.domain_claim_id != Some(claim.id)
        || route.domain_pattern.as_ref() != Some(&claim.pattern)
        || claim.organization_id != route.organization_id
        || claim.project_id != route.project_id
        || claim.environment_id != route.environment_id
        || claim.state != DomainClaimState::Verified
        || claim.aggregate_version == 0
        || claim.failure.is_some()
        || claim
            .verified_at
            .is_none_or(|verified_at| verified_at > observed_at)
        || claim.revoked_at.is_some()
        || claim.updated_at < claim.created_at
        || claim.updated_at > observed_at
        || !claim.covers(&route.hostname)
    {
        return Err(
            "complete Gateway snapshot ordinary Route lacks exact verified domain authority".into(),
        );
    }
    Ok(())
}

fn validate_verified_domain_claim(
    claim: &DomainClaim,
    observed_at: DateTime<Utc>,
) -> Result<(), String> {
    if claim.id.as_uuid().is_nil()
        || claim.organization_id.as_uuid().is_nil()
        || claim.project_id.as_uuid().is_nil()
        || claim.environment_id.as_uuid().is_nil()
        || claim.state != DomainClaimState::Verified
        || claim.aggregate_version == 0
        || claim.failure.is_some()
        || claim
            .verified_at
            .is_none_or(|verified_at| verified_at > observed_at)
        || claim.revoked_at.is_some()
        || claim.updated_at < claim.created_at
        || claim.updated_at > observed_at
    {
        return Err("managed Gateway snapshot DomainClaim lacks exact verified authority".into());
    }
    Ok(())
}

fn validate_route_domain_authority(
    route: &Route,
    claim: &DomainClaim,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> Result<(), String> {
    validate_verified_domain_claim(claim, observed_at)?;
    if route.gateway_node_id != node_id
        || route.domain_claim_id != Some(claim.id)
        || route.domain_pattern.as_ref() != Some(&claim.pattern)
        || route.organization_id != claim.organization_id
        || route.project_id != claim.project_id
        || route.environment_id != claim.environment_id
        || !claim.covers(&route.hostname)
    {
        return Err(
            "managed Gateway snapshot Route does not match its exact DomainClaim authority".into(),
        );
    }
    Ok(())
}

fn insert_domain_claim(
    claims: &mut BTreeMap<DomainClaimId, DomainClaim>,
    claim: DomainClaim,
) -> Result<(), String> {
    match claims.get(&claim.id) {
        Some(existing) if existing != &claim => {
            Err("managed Gateway snapshot observed conflicting versions of one DomainClaim".into())
        }
        Some(_) => Ok(()),
        None => {
            claims.insert(claim.id, claim);
            Ok(())
        }
    }
}

fn insert_claim_authority(
    claims: &mut BTreeMap<
        DomainClaimId,
        (
            OrganizationId,
            ProjectId,
            EnvironmentId,
            u64,
            crate::modules::edge::domain::DomainNamePattern,
        ),
    >,
    domain_claim_id: DomainClaimId,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    aggregate_version: u64,
    pattern: crate::modules::edge::domain::DomainNamePattern,
) -> Result<(), String> {
    if domain_claim_id.as_uuid().is_nil()
        || organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
        || aggregate_version == 0
    {
        return Err("Gateway snapshot DomainClaim version is invalid".into());
    }
    match claims.get(&domain_claim_id) {
        Some((
            existing_organization_id,
            existing_project_id,
            existing_environment_id,
            existing_version,
            existing_pattern,
        )) if *existing_organization_id != organization_id
            || *existing_project_id != project_id
            || *existing_environment_id != environment_id
            || *existing_version != aggregate_version
            || existing_pattern != &pattern =>
        {
            Err("Gateway snapshot observed conflicting versions of one DomainClaim".into())
        }
        Some(_) => Ok(()),
        None => {
            claims.insert(
                domain_claim_id,
                (
                    organization_id,
                    project_id,
                    environment_id,
                    aggregate_version,
                    pattern,
                ),
            );
            Ok(())
        }
    }
}

fn append_mcp_snapshot_acl(
    acl: &mut String,
    content: &McpSnapshotContent<'_>,
    issued_at: DateTime<Utc>,
) -> Result<(), String> {
    const DEFAULT_DENY_SERVICE: &str = "a3s-cloud-mcp-default-deny";
    const DEFAULT_DENY_ENDPOINT: &str = "http://127.0.0.1:9/";

    content.projection.validate(issued_at)?;
    let mut ingress_routes = content.ingress_routes.iter().collect::<Vec<_>>();
    ingress_routes.sort_by_key(|ingress| ingress.route_id());
    let mut routes = content.projection.routes.iter().collect::<Vec<_>>();
    routes.sort_by_key(|route| route.route_id);
    if ingress_routes.len() != routes.len()
        || ingress_routes.iter().zip(&routes).any(|(ingress, route)| {
            ingress.route_id().as_uuid() != route.route_id || ingress.router() != route.router
        })
    {
        return Err("MCP Gateway ingress does not match its complete policy projection".into());
    }
    if routes
        .iter()
        .flat_map(|route| &route.targets)
        .any(|target| target.service == DEFAULT_DENY_SERVICE)
    {
        return Err("MCP target service collides with the fail-closed ingress service".into());
    }

    for ingress in &ingress_routes {
        acl.push_str(&format!(
            "routers {} {{\n  rule = {}\n  service = {}\n  entrypoints = [\"a3s-cloud-https\"]\n}}\n\n",
            acl_string(ingress.router()),
            acl_string(&format!(
                "Host(`{}`) && Path(`{}`)",
                ingress.hostname().as_str(),
                ingress.path()
            )),
            acl_string(DEFAULT_DENY_SERVICE),
        ));
    }
    acl.push_str(&format!(
        "services {} {{\n  load_balancer {{\n    strategy = \"round-robin\"\n    request_timeout = \"1s\"\n    servers = [{{ url = {} }}]\n  }}\n}}\n\n",
        acl_string(DEFAULT_DENY_SERVICE),
        acl_string(DEFAULT_DENY_ENDPOINT),
    ));
    for route in routes {
        let mut targets = route.targets.iter().collect::<Vec<_>>();
        targets.sort_by_key(|target| (target.priority, target.target_id));
        for target in targets {
            acl.push_str(&format!(
                "# MCP target route={} target={} unit={} generation={}\nservices {} {{\n  load_balancer {{\n    strategy = \"round-robin\"\n    request_timeout = {}\n    stream_idle_timeout = {}\n    stream_total_timeout = {}\n    servers = [{{ url = {} }}]\n  }}\n}}\n\n",
                route.route_id,
                target.target_id,
                target.unit_id,
                target.generation,
                acl_string(&target.service),
                acl_string(&route.first_response_timeout),
                acl_string(&route.stream_idle_timeout),
                acl_string(&route.stream_total_timeout),
                acl_string(&target.endpoint),
            ));
        }
    }
    let compiled = McpGatewayProjectionCompiler.compile_at(content.projection, issued_at)?;
    acl.push_str(compiled.acl.trim_end_matches(['\r', '\n']));
    acl.push_str("\n\n");
    Ok(())
}

fn managed_certificate_request(
    certificate_directory: &str,
    certificate_id: GatewayCertificateId,
    dns_names: Vec<String>,
) -> Result<GatewayCertificateRequest, String> {
    let certificate_root = format!(
        "{}/{}",
        certificate_directory.trim_end_matches('/'),
        certificate_id
    );
    let certificate_file = format!("{certificate_root}/certificate.pem");
    let private_key_file = format!("{certificate_root}/private-key.pem");
    GatewayCertificateRequest::new(
        certificate_id.as_uuid(),
        dns_names,
        certificate_file,
        private_key_file,
    )
}

fn acl_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn duration(milliseconds: u64) -> String {
    if milliseconds % 1_000 == 0 {
        format!("{}s", milliseconds / 1_000)
    } else {
        format!("{milliseconds}ms")
    }
}

fn valid_path_prefix(value: &str) -> bool {
    value.starts_with('/') && value.len() <= 255 && !value.contains(['\0', '\r', '\n', '?', '#'])
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || index > 0 && byte.is_ascii_digit()
        })
}

fn valid_certificate_directory(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && value.starts_with('/')
        && !value.split('/').any(|component| component == "..")
}

fn valid_absolute_file(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.contains(['\0', '\r', '\n'])
        && value.starts_with('/')
        && !value.split('/').any(|component| component == "..")
        && value
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .is_some_and(|component| !component.is_empty() && component != ".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::{
        DomainNamePattern, RouteHostname, RoutePath, RoutePortName, RouteTarget, UpstreamEndpoint,
    };
    use crate::modules::shared_kernel::domain::{
        DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, OrganizationId,
        ProjectId, RouteId, WorkloadId, WorkloadRevisionId,
    };
    use chrono::{Duration, Utc};

    fn compiler() -> GatewaySnapshotCompiler {
        GatewaySnapshotCompiler::new(GatewaySnapshotCompilerConfig {
            entrypoint_address: "0.0.0.0:8081".into(),
            management_address: "127.0.0.1:9090".into(),
            management_path_prefix: "/api/gateway".into(),
            management_auth_token_env: "A3S_GATEWAY_ADMIN_TOKEN".into(),
            upstream_request_timeout_ms: 30_000,
            certificate_directory: "/var/lib/a3s-cloud/gateway/certificates".into(),
            managed_state_file: "/var/lib/a3s-gateway/managed-snapshot.json".into(),
        })
        .expect("compiler")
    }

    fn route(node_id: NodeId, hostname: &str, path: &str, port: u16) -> Route {
        let workload_id = WorkloadId::new();
        let workload_revision_id = WorkloadRevisionId::new();
        let now = Utc::now();
        Route::create(
            RouteId::new(),
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            GatewayScopeId::new(),
            node_id,
            RouteHostname::parse(hostname).expect("hostname"),
            RoutePath::parse(path).expect("path"),
            DomainClaimId::new(),
            DomainNamePattern::parse(hostname).expect("domain pattern"),
            GatewayCertificateId::new(),
            workload_id,
            RouteTarget::new(
                workload_id,
                workload_revision_id,
                format!("workload:{workload_id}:revision:{workload_revision_id}"),
                1,
                RoutePortName::parse("http").expect("port"),
                UpstreamEndpoint::parse(format!("http://127.0.0.1:{port}")).expect("upstream"),
                now,
            )
            .expect("target"),
            now,
        )
        .expect("route")
    }

    #[test]
    fn compiles_every_owned_route_into_one_deterministic_snapshot() {
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut first = route(node_id, "z.example.com", "/", 49152);
        first.state = RouteState::Active;
        let mut second = route(node_id, "api.example.com", "/v1", 49153);
        second.gateway_certificate_id = Some(certificate_id);
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        let forward = compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
                certificate_id,
                &[first.clone(), second.clone()],
            )
            .expect("snapshot");
        let reverse = compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 2, Some(1), issued_at, expires_at),
                certificate_id,
                &[second, first],
            )
            .expect("snapshot");
        assert_eq!(forward, reverse);
        assert_eq!(forward.acl.matches("routers \"").count(), 2);
        assert_eq!(forward.acl.matches("services \"").count(), 2);
        assert!(forward
            .acl
            .contains("Host(`api.example.com`) && PathPrefix(`/v1`)"));
        assert!(forward.acl.contains("http://127.0.0.1:49152/"));
        assert!(forward.acl.contains("mode { kind = \"cloud-managed\" }"));
        assert!(forward.acl.contains(&node_id.to_string()));
    }

    #[test]
    fn compiles_certificate_convergence_without_mutating_active_routes() {
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut active = route(node_id, "api.example.com", "/", 49152);
        let previous_certificate_id = active.gateway_certificate_id.expect("previous certificate");
        active.state = RouteState::Active;
        let issued_at = Utc::now();

        let snapshot = compiler()
            .compile_certificate_convergence(
                GatewaySnapshotMetadata::new(
                    node_id,
                    2,
                    Some(1),
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                Some(certificate_id),
                std::slice::from_ref(&active),
            )
            .expect("certificate convergence snapshot");

        assert_eq!(
            active.gateway_certificate_id,
            Some(previous_certificate_id),
            "the replacement is not authoritative before acknowledgement"
        );
        assert_eq!(
            snapshot
                .certificate_request
                .as_ref()
                .map(|request| request.certificate_id),
            Some(certificate_id.as_uuid())
        );
        assert!(snapshot.acl.contains("api.example.com"));
    }

    #[test]
    fn compiles_route_less_revocation_snapshot_without_a_certificate() {
        let node_id = NodeId::new();
        let issued_at = Utc::now();
        let snapshot = compiler()
            .compile_certificate_convergence(
                GatewaySnapshotMetadata::new(
                    node_id,
                    2,
                    Some(1),
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                None,
                &[],
            )
            .expect("route-less revocation snapshot");

        assert!(snapshot.certificate_request.is_none());
        assert!(!snapshot.acl.contains("entrypoints \"a3s-cloud-https\""));
        assert!(snapshot.acl.contains("management {"));
    }

    #[test]
    fn rejects_cross_scope_and_duplicate_route_ownership() {
        let node_id = NodeId::new();
        let first = route(node_id, "api.example.com", "/v1", 49152);
        let duplicate = route(node_id, "api.example.com", "/v1", 49153);
        let issued_at = Utc::now();
        let expires_at = issued_at + Duration::minutes(10);
        assert!(compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
                GatewayCertificateId::new(),
                &[first, duplicate],
            )
            .is_err());
        let foreign = route(NodeId::new(), "other.example.com", "/", 49154);
        assert!(compiler()
            .compile(
                GatewaySnapshotMetadata::new(node_id, 1, None, issued_at, expires_at),
                GatewayCertificateId::new(),
                &[foreign],
            )
            .is_err());
    }

    #[test]
    fn installed_gateway_validates_compiled_snapshot() {
        let Ok(binary) = std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN") else {
            return;
        };
        let node_id = NodeId::new();
        let certificate_id = GatewayCertificateId::new();
        let mut route = route(node_id, "api.example.com", "/v1", 49152);
        route.gateway_certificate_id = Some(certificate_id);
        let issued_at = Utc::now();
        let snapshot = compiler()
            .compile(
                GatewaySnapshotMetadata::new(
                    node_id,
                    1,
                    None,
                    issued_at,
                    issued_at + Duration::minutes(10),
                ),
                certificate_id,
                &[route],
            )
            .expect("snapshot");
        let directory = tempfile::tempdir().expect("Gateway validation directory");
        let path = directory.path().join("gateway.acl");
        std::fs::write(&path, snapshot.acl).expect("write compiled Gateway snapshot");
        let output = std::process::Command::new(binary)
            .arg("validate")
            .arg("--config")
            .arg(path)
            .output()
            .expect("run installed Gateway validator");
        assert!(
            output.status.success(),
            "installed Gateway rejected compiled snapshot: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
