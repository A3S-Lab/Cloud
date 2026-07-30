use crate::modules::edge::domain::events::{GatewayRolloutStaged, RoutePublicationStaged};
use crate::modules::edge::domain::repositories::StageGatewayRollout;
use crate::modules::edge::domain::services::ResolvedRouteTargetSet;
use crate::modules::edge::domain::{
    DomainClaim, DomainNamePattern, GatewayCertificate, GatewayPublication, GatewayRollout,
    GatewayScope, GatewayScopeState, Route, RouteHostname, RoutePath,
};
use crate::modules::edge::infrastructure::{
    CompileManagedGatewayRouteSnapshot, GatewayManagedSnapshotComposition, GatewaySnapshotCompiler,
    GatewaySnapshotMetadata, GatewaySnapshotPublicationOwner, PlannedGatewayNodeDesiredState,
    StageManagedGatewayRollout,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, GatewayRolloutId, IdempotencyRequest,
    NodeCommandId, NodeId, RouteId,
};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GatewayMemberSnapshotContext {
    pub scope: GatewayScopeState,
    pub active_routes: Vec<Route>,
}

#[derive(Debug, Clone)]
pub struct CompileGatewayRouteRollout {
    pub scope: GatewayScope,
    pub rollout_id: GatewayRolloutId,
    pub generation: u64,
    pub correlation_id: Uuid,
    pub route_id: RouteId,
    pub hostname: RouteHostname,
    pub path_prefix: RoutePath,
    pub domain_claim_id: DomainClaimId,
    pub domain_pattern: DomainNamePattern,
    pub target_set: ResolvedRouteTargetSet,
    pub member_contexts: Vec<GatewayMemberSnapshotContext>,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CompileManagedGatewayRouteRollout {
    pub scope: GatewayScope,
    pub rollout_id: GatewayRolloutId,
    pub generation: u64,
    pub correlation_id: Uuid,
    pub route_id: RouteId,
    pub hostname: RouteHostname,
    pub path_prefix: RoutePath,
    pub domain_claim: DomainClaim,
    pub target_set: ResolvedRouteTargetSet,
    pub member_desired_states: Vec<PlannedGatewayNodeDesiredState>,
    pub issued_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CompiledGatewayRouteRollout {
    pub scope: GatewayScope,
    pub rollout: GatewayRollout,
    pub route_replicas: Vec<Route>,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub expected_scope_versions: BTreeMap<NodeId, u64>,
    pub managed_compositions: BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
}

impl CompiledGatewayRouteRollout {
    pub fn primary_route(&self) -> Result<&Route, String> {
        self.route_replicas
            .iter()
            .find(|route| route.gateway_node_id == self.scope.node_id)
            .ok_or_else(|| "compiled Gateway rollout omitted its bootstrap primary route".into())
    }

    pub fn stage_bundle(
        &self,
        idempotency: IdempotencyRequest,
    ) -> Result<StageGatewayRollout, String> {
        let primary_route = self.primary_route()?;
        let primary_publication = self
            .publications
            .iter()
            .find(|publication| publication.node_id == self.scope.node_id)
            .ok_or_else(|| {
                "compiled Gateway rollout omitted its primary publication".to_string()
            })?;
        let bundle = StageGatewayRollout {
            scope: self.scope.clone(),
            rollout: self.rollout.clone(),
            route_replicas: self.route_replicas.clone(),
            publications: self.publications.clone(),
            certificates: self.certificates.clone(),
            expected_scope_versions: self.expected_scope_versions.clone(),
            idempotency,
            event: GatewayRolloutStaged::envelope(&self.scope, &self.rollout)
                .map_err(|error| error.to_string())?,
            route_event: Some(RoutePublicationStaged::envelope(
                primary_route,
                primary_publication,
            )?),
        };
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn managed_stage_bundle(
        &self,
        idempotency: IdempotencyRequest,
    ) -> Result<StageManagedGatewayRollout, String> {
        StageManagedGatewayRollout::new(
            self.stage_bundle(idempotency)?,
            self.managed_compositions.clone(),
        )
    }
}

#[derive(Debug, Clone)]
pub struct GatewayRouteRolloutCompiler {
    snapshots: GatewaySnapshotCompiler,
    command_ttl: Duration,
    snapshot_ttl: Duration,
}

impl GatewayRouteRolloutCompiler {
    pub fn new(
        snapshots: GatewaySnapshotCompiler,
        command_ttl: Duration,
        snapshot_ttl: Duration,
    ) -> Result<Self, String> {
        if command_ttl <= Duration::zero()
            || snapshot_ttl < command_ttl
            || snapshot_ttl > Duration::days(30)
        {
            return Err(
                "Gateway rollout validity requires a positive command TTL and a bounded snapshot TTL"
                    .into(),
            );
        }
        Ok(Self {
            snapshots,
            command_ttl,
            snapshot_ttl,
        })
    }

    pub fn compile(
        &self,
        request: CompileGatewayRouteRollout,
    ) -> Result<CompiledGatewayRouteRollout, String> {
        request.scope.validate()?;
        if request.rollout_id.as_uuid().is_nil()
            || request.generation == 0
            || request.correlation_id.is_nil()
            || request.route_id.as_uuid().is_nil()
            || request.domain_claim_id.as_uuid().is_nil()
        {
            return Err("Gateway route rollout identities must be positive".into());
        }
        if !request.domain_pattern.covers(&request.hostname) {
            return Err("Gateway route rollout domain claim does not cover its hostname".into());
        }
        let issued_at = canonical_timestamp(request.issued_at);
        let command_not_after = issued_at
            .checked_add_signed(self.command_ttl)
            .ok_or_else(|| "Gateway rollout command expiry exceeds supported time".to_string())?;
        let snapshot_expires_at = issued_at
            .checked_add_signed(self.snapshot_ttl)
            .ok_or_else(|| "Gateway rollout snapshot expiry exceeds supported time".to_string())?;

        let target_nodes = request
            .target_set
            .targets()
            .iter()
            .map(|target| target.node_id)
            .collect::<BTreeSet<_>>();
        let desired_nodes = request
            .scope
            .member_node_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if target_nodes != desired_nodes {
            return Err(
                "Gateway route rollout target set does not match desired scope membership".into(),
            );
        }
        let contexts = canonical_contexts(request.member_contexts, &desired_nodes)?;

        let mut route_replicas = Vec::with_capacity(contexts.len());
        let mut publications = Vec::with_capacity(contexts.len());
        let mut certificates = Vec::with_capacity(contexts.len());
        let mut expected_scope_versions = BTreeMap::new();
        for context in contexts {
            let node_id = context.scope.node_id;
            let resolved_target = request
                .target_set
                .for_member(node_id)
                .ok_or_else(|| "Gateway rollout member has no healthy route target".to_string())?;
            if context.active_routes.iter().any(|route| {
                route.gateway_node_id != node_id
                    || route.organization_id != request.scope.organization_id
            }) {
                return Err(
                    "Gateway member snapshot contains a route from another node or organization"
                        .into(),
                );
            }
            let certificate_id = GatewayCertificateId::new();
            let mut route = Route::create(
                request.route_id,
                request.scope.organization_id,
                request.scope.project_id,
                request.scope.environment_id,
                request.scope.id,
                node_id,
                request.hostname.clone(),
                request.path_prefix.clone(),
                request.domain_claim_id,
                request.domain_pattern.clone(),
                certificate_id,
                resolved_target.workload_id,
                resolved_target.target.clone(),
                issued_at,
            )?;
            let revision = context.scope.next_revision()?;
            let mut complete_routes = context.active_routes;
            complete_routes.push(route.clone());
            let snapshot = self.snapshots.compile(
                GatewaySnapshotMetadata::new(
                    node_id,
                    revision,
                    context.scope.installed_revision,
                    issued_at,
                    snapshot_expires_at,
                ),
                certificate_id,
                &complete_routes,
            )?;
            let command_id = NodeCommandId::new();
            route.stage(
                revision,
                command_id,
                snapshot.snapshot_digest.clone(),
                issued_at,
            )?;
            let publication = GatewayPublication::stage(
                node_id,
                command_id,
                request.correlation_id,
                snapshot,
                issued_at,
                command_not_after,
            )?;
            let certificate_request = publication.certificate_request.clone().ok_or_else(|| {
                "TLS Gateway rollout publication omitted its certificate request".to_string()
            })?;
            let mut domain_claim_ids = complete_routes
                .iter()
                .filter_map(|route| route.domain_claim_id)
                .collect::<Vec<_>>();
            domain_claim_ids.sort();
            domain_claim_ids.dedup();
            let certificate = GatewayCertificate::provision(
                certificate_id,
                request.scope.organization_id,
                node_id,
                domain_claim_ids,
                revision,
                command_id,
                publication.snapshot_digest.clone(),
                certificate_request,
                issued_at,
            )?;
            expected_scope_versions.insert(node_id, context.scope.aggregate_version);
            route_replicas.push(route);
            publications.push(publication);
            certificates.push(certificate);
        }

        route_replicas.sort_by_key(|route| route.gateway_node_id);
        publications.sort_by_key(|publication| publication.node_id);
        certificates.sort_by_key(|certificate| certificate.node_id);
        let rollout = GatewayRollout::stage(
            request.rollout_id,
            &request.scope,
            request.generation,
            &publications,
            issued_at,
        )?;
        Ok(CompiledGatewayRouteRollout {
            scope: request.scope,
            rollout,
            route_replicas,
            publications,
            certificates,
            expected_scope_versions,
            managed_compositions: BTreeMap::new(),
        })
    }

    pub fn compile_managed(
        &self,
        request: CompileManagedGatewayRouteRollout,
    ) -> Result<CompiledGatewayRouteRollout, String> {
        request.scope.validate()?;
        if request.rollout_id.as_uuid().is_nil()
            || request.generation == 0
            || request.correlation_id.is_nil()
            || request.route_id.as_uuid().is_nil()
            || request.domain_claim.id.as_uuid().is_nil()
        {
            return Err("Gateway route rollout identities must be positive".into());
        }
        if !request.domain_claim.covers(&request.hostname)
            || request.domain_claim.organization_id != request.scope.organization_id
            || request.domain_claim.project_id != request.scope.project_id
            || request.domain_claim.environment_id != request.scope.environment_id
        {
            return Err("Gateway route rollout DomainClaim does not cover its exact tenant".into());
        }
        let issued_at = canonical_timestamp(request.issued_at);
        let command_not_after = issued_at
            .checked_add_signed(self.command_ttl)
            .ok_or_else(|| "Gateway rollout command expiry exceeds supported time".to_string())?;
        let snapshot_expires_at = issued_at
            .checked_add_signed(self.snapshot_ttl)
            .ok_or_else(|| "Gateway rollout snapshot expiry exceeds supported time".to_string())?;
        let target_nodes = request
            .target_set
            .targets()
            .iter()
            .map(|target| target.node_id)
            .collect::<BTreeSet<_>>();
        let desired_nodes = request
            .scope
            .member_node_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if target_nodes != desired_nodes {
            return Err(
                "Gateway route rollout target set does not match desired scope membership".into(),
            );
        }
        let contexts = canonical_managed_contexts(request.member_desired_states, &desired_nodes)?;

        let mut route_replicas = Vec::with_capacity(contexts.len());
        let mut publications = Vec::with_capacity(contexts.len());
        let mut certificates = Vec::with_capacity(contexts.len());
        let mut expected_scope_versions = BTreeMap::new();
        let mut managed_compositions = BTreeMap::new();
        for desired_state in contexts {
            let node_id = desired_state.physical_scope().node_id;
            let resolved_target = request
                .target_set
                .for_member(node_id)
                .ok_or_else(|| "Gateway rollout member has no healthy route target".to_string())?;
            if desired_state.active_routes().iter().any(|input| {
                input.route.gateway_node_id != node_id
                    || input.route.organization_id != request.scope.organization_id
            }) {
                return Err(
                    "Gateway member desired state contains a Route from another node or organization"
                        .into(),
                );
            }
            let certificate_id = GatewayCertificateId::new();
            let mut route = Route::create(
                request.route_id,
                request.scope.organization_id,
                request.scope.project_id,
                request.scope.environment_id,
                request.scope.id,
                node_id,
                request.hostname.clone(),
                request.path_prefix.clone(),
                request.domain_claim.id,
                request.domain_claim.pattern.clone(),
                certificate_id,
                resolved_target.workload_id,
                resolved_target.target.clone(),
                issued_at,
            )?;
            let revision = desired_state.physical_scope().next_revision()?;
            let expected_scope_version = desired_state.physical_scope().aggregate_version;
            let mut complete_routes = desired_state
                .active_routes()
                .iter()
                .map(|input| input.route.clone())
                .collect::<Vec<_>>();
            complete_routes.push(route.clone());
            let candidate = self.snapshots.compile_managed_route_snapshot(
                CompileManagedGatewayRouteSnapshot {
                    metadata: GatewaySnapshotMetadata::new(
                        node_id,
                        revision,
                        desired_state.physical_scope().installed_revision,
                        issued_at,
                        snapshot_expires_at,
                    ),
                    desired_state,
                    certificate_id,
                    snapshot_routes: complete_routes,
                    additional_domain_claims: vec![request.domain_claim.clone()],
                },
            )?;
            let command_id = NodeCommandId::new();
            route.stage(
                revision,
                command_id,
                candidate.snapshot().snapshot_digest.clone(),
                issued_at,
            )?;
            let publication = GatewayPublication::stage(
                node_id,
                command_id,
                request.correlation_id,
                candidate.snapshot().clone(),
                issued_at,
                command_not_after,
            )?;
            let certificate_request = publication.certificate_request.clone().ok_or_else(|| {
                "TLS Gateway rollout publication omitted its certificate request".to_string()
            })?;
            let domain_claim_ids = candidate
                .domain_claim_versions()
                .iter()
                .map(|version| version.domain_claim_id())
                .collect();
            let certificate = GatewayCertificate::provision(
                certificate_id,
                request.scope.organization_id,
                node_id,
                domain_claim_ids,
                revision,
                command_id,
                publication.snapshot_digest.clone(),
                certificate_request,
                issued_at,
            )?;
            let composition = GatewayManagedSnapshotComposition::new(
                candidate,
                &publication,
                GatewaySnapshotPublicationOwner::Ordinary,
            )?;
            expected_scope_versions.insert(node_id, expected_scope_version);
            managed_compositions.insert(node_id, composition);
            route_replicas.push(route);
            publications.push(publication);
            certificates.push(certificate);
        }

        route_replicas.sort_by_key(|route| route.gateway_node_id);
        publications.sort_by_key(|publication| publication.node_id);
        certificates.sort_by_key(|certificate| certificate.node_id);
        let rollout = GatewayRollout::stage(
            request.rollout_id,
            &request.scope,
            request.generation,
            &publications,
            issued_at,
        )?;
        Ok(CompiledGatewayRouteRollout {
            scope: request.scope,
            rollout,
            route_replicas,
            publications,
            certificates,
            expected_scope_versions,
            managed_compositions,
        })
    }
}

fn canonical_contexts(
    mut contexts: Vec<GatewayMemberSnapshotContext>,
    desired_nodes: &BTreeSet<NodeId>,
) -> Result<Vec<GatewayMemberSnapshotContext>, String> {
    contexts.sort_by_key(|context| context.scope.node_id);
    if contexts.len() != desired_nodes.len()
        || contexts
            .windows(2)
            .any(|contexts| contexts[0].scope.node_id == contexts[1].scope.node_id)
        || contexts
            .iter()
            .map(|context| context.scope.node_id)
            .collect::<BTreeSet<_>>()
            != *desired_nodes
    {
        return Err(
            "Gateway route rollout contexts must cover every desired member exactly once".into(),
        );
    }
    Ok(contexts)
}

fn canonical_managed_contexts(
    mut contexts: Vec<PlannedGatewayNodeDesiredState>,
    desired_nodes: &BTreeSet<NodeId>,
) -> Result<Vec<PlannedGatewayNodeDesiredState>, String> {
    contexts.sort_by_key(|context| context.physical_scope().node_id);
    if contexts.len() != desired_nodes.len()
        || contexts.windows(2).any(|contexts| {
            contexts[0].physical_scope().node_id == contexts[1].physical_scope().node_id
        })
        || contexts
            .iter()
            .map(|context| context.physical_scope().node_id)
            .collect::<BTreeSet<_>>()
            != *desired_nodes
    {
        return Err(
            "managed Gateway route rollout contexts must cover every desired member exactly once"
                .into(),
        );
    }
    Ok(contexts)
}
