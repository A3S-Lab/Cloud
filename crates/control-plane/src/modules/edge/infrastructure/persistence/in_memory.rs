use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayRolloutDispatchTarget, GatewayRolloutResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRollout, StageGatewayRouteCutover,
    StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayPublication, GatewayPublicationState, GatewayRollout, GatewayRouteCutover, GatewayScope,
    GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, IdempotentWrite, NodeCommandId, NodeId, OrganizationId, ProjectId,
    RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

mod acknowledgements;
mod certificate_convergence;
mod certificates;
mod gateway_scopes;
mod rollouts;

#[derive(Default)]
pub struct InMemoryEdgeRepository {
    state: RwLock<State>,
}

#[derive(Clone, Default)]
struct State {
    gateway_scopes: BTreeMap<GatewayScopeId, GatewayScope>,
    gateway_scope_bindings:
        BTreeMap<(OrganizationId, ProjectId, EnvironmentId, NodeId), GatewayScopeId>,
    gateway_scope_idempotency: BTreeMap<(String, String), (String, GatewayScope)>,
    domain_claims: BTreeMap<DomainClaimId, DomainClaim>,
    domain_idempotency: BTreeMap<(String, String), (String, DomainClaim)>,
    scopes: BTreeMap<NodeId, GatewayScopeState>,
    routes: BTreeMap<RouteId, Route>,
    ownership: BTreeMap<(NodeId, String, String), RouteId>,
    publications: BTreeMap<(NodeId, u64), GatewayPublication>,
    certificates: BTreeMap<GatewayCertificateId, GatewayCertificate>,
    certificate_convergences: BTreeMap<(NodeId, u64), GatewayCertificateConvergence>,
    cutovers: BTreeMap<DeploymentId, GatewayRouteCutover>,
    commands: BTreeMap<(NodeId, NodeCommandId), u64>,
    idempotency: BTreeMap<(String, String), (String, EdgeRoutePublicationResult)>,
    cutover_idempotency: BTreeMap<(String, String), (String, GatewayRouteCutoverResult)>,
    rollouts: BTreeMap<GatewayRolloutId, GatewayRollout>,
    rollout_publications: BTreeMap<(NodeId, u64), GatewayRolloutId>,
    rollout_idempotency: BTreeMap<(String, String), (String, GatewayRolloutResult)>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryEdgeRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }
}

#[async_trait]
impl IEdgeRepository for InMemoryEdgeRepository {
    async fn create_gateway_scope(
        &self,
        bundle: CreateGatewayScopeWrite,
    ) -> Result<IdempotentWrite<GatewayScope>, RepositoryError> {
        let mut state = self.state.write().await;
        gateway_scopes::create(&mut state, bundle)
    }

    async fn find_gateway_scope(
        &self,
        organization_id: OrganizationId,
        scope_id: GatewayScopeId,
    ) -> Result<GatewayScope, RepositoryError> {
        let state = self.state.read().await;
        gateway_scopes::find(&state, organization_id, scope_id)
    }

    async fn list_gateway_scopes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GatewayScope>, RepositoryError> {
        let state = self.state.read().await;
        Ok(gateway_scopes::list(
            &state,
            organization_id,
            project_id,
            environment_id,
        ))
    }

    async fn replay_domain_claim_write(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<DomainClaim>, RepositoryError> {
        let state = self.state.read().await;
        let Some((digest, claim)) = state
            .domain_idempotency
            .get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        Ok(Some(claim.clone()))
    }

    async fn create_domain_claim(
        &self,
        bundle: CreateDomainClaimWrite,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
        validate_domain_event(&bundle.claim, &bundle.event)?;
        let mut state = self.state.write().await;
        let key = (
            bundle.idempotency.scope.clone(),
            bundle.idempotency.key.clone(),
        );
        if let Some((digest, claim)) = state.domain_idempotency.get(&key) {
            if digest != &bundle.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: claim.clone(),
                replayed: true,
            });
        }
        if state.domain_claims.values().any(|existing| {
            matches!(
                existing.state,
                DomainClaimState::Pending | DomainClaimState::Verified
            ) && existing.pattern.conflicts_with(&bundle.claim.pattern)
        }) {
            return Err(RepositoryError::Conflict(
                "domain pattern overlaps an existing ownership claim".into(),
            ));
        }
        if state.domain_claims.contains_key(&bundle.claim.id) {
            return Err(RepositoryError::Conflict(
                "domain claim identity already exists".into(),
            ));
        }
        state
            .domain_claims
            .insert(bundle.claim.id, bundle.claim.clone());
        state.domain_idempotency.insert(
            key,
            (bundle.idempotency.request_digest, bundle.claim.clone()),
        );
        state.outbox.push(bundle.event);
        Ok(IdempotentWrite {
            value: bundle.claim,
            replayed: false,
        })
    }

    async fn transition_domain_claim(
        &self,
        bundle: TransitionDomainClaim,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError> {
        validate_domain_event(&bundle.claim, &bundle.event)?;
        let mut state = self.state.write().await;
        let key = (
            bundle.idempotency.scope.clone(),
            bundle.idempotency.key.clone(),
        );
        if let Some((digest, claim)) = state.domain_idempotency.get(&key) {
            if digest != &bundle.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: claim.clone(),
                replayed: true,
            });
        }
        let existing = state
            .domain_claims
            .get(&bundle.claim.id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        if existing.aggregate_version != bundle.expected_version
            || bundle.claim.aggregate_version != bundle.expected_version + 1
            || existing.organization_id != bundle.claim.organization_id
            || existing.project_id != bundle.claim.project_id
            || existing.environment_id != bundle.claim.environment_id
            || existing.pattern != bundle.claim.pattern
            || existing.challenge_dns_name != bundle.claim.challenge_dns_name
            || existing.challenge_value != bundle.claim.challenge_value
            || existing.created_at != bundle.claim.created_at
        {
            return Err(RepositoryError::Conflict(
                "domain claim changed while applying its transition".into(),
            ));
        }
        state
            .domain_claims
            .insert(bundle.claim.id, bundle.claim.clone());
        state.domain_idempotency.insert(
            key,
            (bundle.idempotency.request_digest, bundle.claim.clone()),
        );
        state.outbox.push(bundle.event);
        Ok(IdempotentWrite {
            value: bundle.claim,
            replayed: false,
        })
    }

    async fn find_domain_claim(
        &self,
        organization_id: OrganizationId,
        claim_id: DomainClaimId,
    ) -> Result<DomainClaim, RepositoryError> {
        self.state
            .read()
            .await
            .domain_claims
            .get(&claim_id)
            .filter(|claim| claim.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_domain_claims(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<DomainClaim>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .domain_claims
            .values()
            .filter(|claim| {
                claim.organization_id == organization_id
                    && claim.project_id == project_id
                    && claim.environment_id == environment_id
            })
            .cloned()
            .collect())
    }

    async fn replay_route_publication(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<EdgeRoutePublicationResult>, RepositoryError> {
        let state = self.state.read().await;
        let Some((digest, existing)) = state
            .idempotency
            .get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        let mut replay = existing.clone();
        replay.replayed = true;
        Ok(Some(replay))
    }

    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .scopes
            .get(&node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(node_id)))
    }

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .routes
            .values()
            .filter(|route| route.gateway_node_id == node_id && route.state == RouteState::Active)
            .cloned()
            .collect())
    }

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            bundle.idempotency.scope.clone(),
            bundle.idempotency.key.clone(),
        );
        if let Some((digest, existing)) = state.idempotency.get(&idempotency_key) {
            if digest != &bundle.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut replay = existing.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        gateway_scopes::validate_route_binding(&state, &bundle.gateway_scope, &bundle.route)?;
        let current = state
            .scopes
            .get(&bundle.publication.node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(bundle.publication.node_id));
        if current.aggregate_version != bundle.expected_scope_version {
            return Err(RepositoryError::Conflict(
                "Gateway scope changed while compiling the complete snapshot".into(),
            ));
        }
        if state.publications.values().any(|publication| {
            publication.node_id == bundle.publication.node_id
                && publication.state == GatewayPublicationState::Pending
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway scope already has a pending complete snapshot".into(),
            ));
        }
        if bundle.publication.revision
            != current.next_revision().map_err(RepositoryError::Conflict)?
            || bundle.publication.expected_revision != current.installed_revision
        {
            return Err(RepositoryError::Conflict(
                "Gateway publication does not advance the authoritative scope revision".into(),
            ));
        }
        let ownership = (
            bundle.route.gateway_node_id,
            bundle.route.hostname.as_str().to_owned(),
            bundle.route.path_prefix.as_str().to_owned(),
        );
        if state.ownership.contains_key(&ownership) || state.routes.contains_key(&bundle.route.id) {
            return Err(RepositoryError::Conflict(
                "hostname and path are already owned in this Gateway scope".into(),
            ));
        }
        if state.certificates.contains_key(&bundle.certificate.id) {
            return Err(RepositoryError::Conflict(
                "Gateway certificate identity already exists".into(),
            ));
        }
        let result = EdgeRoutePublicationResult {
            route: bundle.route.clone(),
            certificate: bundle.certificate.clone(),
            publication: bundle.publication.clone(),
            replayed: false,
        };
        state.ownership.insert(ownership, bundle.route.id);
        state.routes.insert(bundle.route.id, bundle.route);
        state
            .certificates
            .insert(bundle.certificate.id, bundle.certificate);
        state.publications.insert(
            (bundle.publication.node_id, bundle.publication.revision),
            bundle.publication.clone(),
        );
        state.commands.insert(
            (bundle.publication.node_id, bundle.publication.command_id),
            bundle.publication.revision,
        );
        state.scopes.insert(
            bundle.publication.node_id,
            GatewayScopeState {
                node_id: bundle.publication.node_id,
                last_issued_revision: bundle.publication.revision,
                installed_revision: current.installed_revision,
                aggregate_version: current.aggregate_version + 1,
            },
        );
        state.idempotency.insert(
            idempotency_key,
            (bundle.idempotency.request_digest, result.clone()),
        );
        state.outbox.push(bundle.event);
        Ok(result)
    }

    async fn replay_gateway_route_cutover(
        &self,
        idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
    ) -> Result<Option<GatewayRouteCutoverResult>, RepositoryError> {
        let state = self.state.read().await;
        let Some((digest, existing)) = state
            .cutover_idempotency
            .get(&(idempotency.scope.clone(), idempotency.key.clone()))
        else {
            return Ok(None);
        };
        if digest != &idempotency.request_digest {
            return Err(RepositoryError::IdempotencyConflict);
        }
        let mut replay = existing.clone();
        replay.replayed = true;
        Ok(Some(replay))
    }

    async fn stage_gateway_route_cutover(
        &self,
        bundle: StageGatewayRouteCutover,
    ) -> Result<GatewayRouteCutoverResult, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        let mut state = self.state.write().await;
        let idempotency_key = (
            bundle.idempotency.scope.clone(),
            bundle.idempotency.key.clone(),
        );
        if let Some((digest, existing)) = state.cutover_idempotency.get(&idempotency_key) {
            if digest != &bundle.idempotency.request_digest {
                return Err(RepositoryError::IdempotencyConflict);
            }
            let mut replay = existing.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        let current = state
            .scopes
            .get(&bundle.publication.node_id)
            .cloned()
            .unwrap_or_else(|| GatewayScopeState::empty(bundle.publication.node_id));
        if current.aggregate_version != bundle.expected_scope_version {
            return Err(RepositoryError::Conflict(
                "Gateway scope changed while compiling the route cutover snapshot".into(),
            ));
        }
        if state.publications.values().any(|publication| {
            publication.node_id == bundle.publication.node_id
                && publication.state == GatewayPublicationState::Pending
        }) {
            return Err(RepositoryError::Conflict(
                "Gateway scope already has a pending complete snapshot".into(),
            ));
        }
        if bundle.publication.revision
            != current.next_revision().map_err(RepositoryError::Conflict)?
            || bundle.publication.expected_revision != current.installed_revision
        {
            return Err(RepositoryError::Conflict(
                "Gateway route cutover does not advance the authoritative scope revision".into(),
            ));
        }
        if state.cutovers.contains_key(&bundle.cutover.deployment_id)
            || state.certificates.contains_key(&bundle.certificate.id)
        {
            return Err(RepositoryError::Conflict(
                "Gateway route cutover identity already exists".into(),
            ));
        }
        gateway_scopes::validate_cutover_bindings(&state, &bundle.cutover.routes)?;
        validate_pending_cutover_routes(&state.routes, &bundle.cutover)?;

        let result = GatewayRouteCutoverResult {
            cutover: bundle.cutover.clone(),
            certificate: bundle.certificate.clone(),
            publication: bundle.publication.clone(),
            replayed: false,
        };
        state
            .certificates
            .insert(bundle.certificate.id, bundle.certificate);
        state.publications.insert(
            (bundle.publication.node_id, bundle.publication.revision),
            bundle.publication.clone(),
        );
        state.commands.insert(
            (bundle.publication.node_id, bundle.publication.command_id),
            bundle.publication.revision,
        );
        state.scopes.insert(
            bundle.publication.node_id,
            GatewayScopeState {
                node_id: bundle.publication.node_id,
                last_issued_revision: bundle.publication.revision,
                installed_revision: current.installed_revision,
                aggregate_version: current.aggregate_version + 1,
            },
        );
        state
            .cutovers
            .insert(bundle.cutover.deployment_id, bundle.cutover);
        state.cutover_idempotency.insert(
            idempotency_key,
            (bundle.idempotency.request_digest, result.clone()),
        );
        state.outbox.push(bundle.event);
        Ok(result)
    }

    async fn stage_gateway_rollout(
        &self,
        bundle: StageGatewayRollout,
    ) -> Result<GatewayRolloutResult, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::stage(&mut state, bundle)
    }

    async fn pending_gateway_rollout_dispatches(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError> {
        let state = self.state.read().await;
        rollouts::pending_dispatches(&state, limit)
    }

    async fn find_gateway_rollout(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRollout, RepositoryError> {
        let state = self.state.read().await;
        rollouts::find(&state, organization_id, rollout_id)
    }

    async fn mark_gateway_rollout_replica_unavailable(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::mark_unavailable(
            &mut state,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            failure,
            observed_at,
        )
    }

    async fn gateway_certificate_convergence_targets(
        &self,
        certificate_renew_before: DateTime<Utc>,
        snapshot_renew_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError> {
        let state = self.state.read().await;
        certificate_convergence::targets(
            &state,
            certificate_renew_before,
            snapshot_renew_before,
            limit,
        )
    }

    async fn pending_gateway_certificate_convergences(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError> {
        let state = self.state.read().await;
        certificate_convergence::pending(&state, limit)
    }

    async fn stage_gateway_certificate_convergence(
        &self,
        bundle: StageGatewayCertificateConvergence,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
        let mut state = self.state.write().await;
        certificate_convergence::stage(&mut state, bundle)
    }

    async fn find_gateway_certificate_convergence(
        &self,
        node_id: NodeId,
        gateway_revision: u64,
    ) -> Result<Option<GatewayCertificateConvergence>, RepositoryError> {
        let state = self.state.read().await;
        Ok(certificate_convergence::find(
            &state,
            node_id,
            gateway_revision,
        ))
    }

    async fn obsolete_gateway_certificates(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError> {
        let state = self.state.read().await;
        certificate_convergence::obsolete(&state, limit)
    }

    async fn find_gateway_route_cutover(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Option<GatewayRouteCutover>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .cutovers
            .get(&deployment_id)
            .filter(|cutover| cutover.organization_id == organization_id)
            .cloned())
    }

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError> {
        self.state
            .read()
            .await
            .routes
            .get(&route_id)
            .filter(|route| route.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .routes
            .values()
            .filter(|route| {
                route.organization_id == organization_id
                    && route.project_id == project_id
                    && route.environment_id == environment_id
            })
            .cloned()
            .collect())
    }

    async fn find_gateway_certificate(
        &self,
        node_id: NodeId,
        certificate_id: GatewayCertificateId,
    ) -> Result<GatewayCertificate, RepositoryError> {
        self.state
            .read()
            .await
            .certificates
            .get(&certificate_id)
            .filter(|certificate| certificate.node_id == node_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn list_gateway_certificates(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError> {
        Ok(self
            .state
            .read()
            .await
            .certificates
            .values()
            .filter(|certificate| certificate.organization_id == organization_id)
            .cloned()
            .collect())
    }

    async fn transition_gateway_certificate(
        &self,
        certificate: GatewayCertificate,
        expected_version: u64,
    ) -> Result<GatewayCertificate, RepositoryError> {
        let mut state = self.state.write().await;
        let existing = state
            .certificates
            .get(&certificate.id)
            .ok_or(RepositoryError::NotFound)?;
        certificates::validate_transition(existing, &certificate, expected_version)?;
        state
            .certificates
            .insert(certificate.id, certificate.clone());
        Ok(certificate)
    }

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        acknowledgements::project(&self.state, acknowledgement, received_at).await
    }
}

fn validate_pending_cutover_routes(
    routes: &BTreeMap<RouteId, Route>,
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    let active_ids = routes
        .values()
        .filter(|route| {
            route.state == RouteState::Active
                && route.organization_id == cutover.organization_id
                && route.workload_id == cutover.workload_id
        })
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    let candidate_ids = cutover
        .routes
        .iter()
        .map(|route| route.id)
        .collect::<BTreeSet<_>>();
    if active_ids.is_empty() || active_ids != candidate_ids {
        return Err(RepositoryError::Conflict(
            "Gateway route cutover must replace every active route for the previous revision"
                .into(),
        ));
    }
    for candidate in &cutover.routes {
        let current = routes.get(&candidate.id).ok_or(RepositoryError::NotFound)?;
        if !same_route_ownership(current, candidate)
            || current.state != RouteState::Active
            || current.target.workload_revision_id != cutover.previous_revision_id
            || current.target.runtime_generation != cutover.previous_generation
            || current.gateway_node_id != cutover.node_id
            || candidate.state != RouteState::Publishing
            || candidate.target.workload_revision_id != cutover.candidate_revision_id
            || candidate.target.runtime_generation != cutover.candidate_generation
            || candidate.gateway_certificate_id == current.gateway_certificate_id
            || candidate.aggregate_version != current.aggregate_version.saturating_add(1)
            || candidate.updated_at < current.updated_at
        {
            return Err(RepositoryError::Conflict(
                "active route changed while staging its Gateway cutover".into(),
            ));
        }
    }
    Ok(())
}

fn validate_applied_cutover_routes(
    routes: &BTreeMap<RouteId, Route>,
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    for candidate in &cutover.routes {
        let current = routes
            .get(&candidate.id)
            .ok_or_else(|| RepositoryError::Storage("cutover route disappeared".into()))?;
        if !same_route_ownership(current, candidate)
            || current.state != RouteState::Active
            || current.target.workload_revision_id != cutover.previous_revision_id
            || current.target.runtime_generation != cutover.previous_generation
            || candidate.state != RouteState::Active
            || candidate.target.workload_revision_id != cutover.candidate_revision_id
            || candidate.target.runtime_generation != cutover.candidate_generation
            || candidate.aggregate_version != current.aggregate_version.saturating_add(2)
            || candidate.updated_at < current.updated_at
        {
            return Err(RepositoryError::Conflict(
                "active route changed before applying its Gateway cutover".into(),
            ));
        }
    }
    Ok(())
}

fn same_route_ownership(current: &Route, candidate: &Route) -> bool {
    current.id == candidate.id
        && current.organization_id == candidate.organization_id
        && current.project_id == candidate.project_id
        && current.environment_id == candidate.environment_id
        && current.gateway_scope_id == candidate.gateway_scope_id
        && current.gateway_node_id == candidate.gateway_node_id
        && current.hostname == candidate.hostname
        && current.path_prefix == candidate.path_prefix
        && current.domain_claim_id == candidate.domain_claim_id
        && current.domain_pattern == candidate.domain_pattern
        && current.workload_id == candidate.workload_id
        && current.target.port_name == candidate.target.port_name
        && current.created_at == candidate.created_at
}

fn validate_domain_event(
    claim: &DomainClaim,
    event: &DomainEventEnvelope,
) -> Result<(), RepositoryError> {
    if event.organization_id != claim.organization_id.as_uuid()
        || event.aggregate_id != claim.id.as_uuid()
        || event.aggregate_version != claim.aggregate_version
        || event.correlation_id.is_nil()
        || event.event_id.is_nil()
        || event.schema_version == 0
        || event.event_key.trim().is_empty()
    {
        return Err(RepositoryError::Conflict(
            "domain claim event does not match its aggregate".into(),
        ));
    }
    Ok(())
}
