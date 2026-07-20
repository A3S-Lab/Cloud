use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, EdgeRoutePublicationResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayRouteCutover, StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayCertificate, GatewayPublication, GatewayPublicationState,
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId,
    IdempotentWrite, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, GatewayAckState, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct InMemoryEdgeRepository {
    state: RwLock<State>,
}

#[derive(Default)]
struct State {
    domain_claims: BTreeMap<DomainClaimId, DomainClaim>,
    domain_idempotency: BTreeMap<(String, String), (String, DomainClaim)>,
    scopes: BTreeMap<NodeId, GatewayScopeState>,
    routes: BTreeMap<RouteId, Route>,
    ownership: BTreeMap<(NodeId, String, String), RouteId>,
    publications: BTreeMap<(NodeId, u64), GatewayPublication>,
    certificates: BTreeMap<GatewayCertificateId, GatewayCertificate>,
    cutovers: BTreeMap<DeploymentId, GatewayRouteCutover>,
    commands: BTreeMap<(NodeId, NodeCommandId), u64>,
    idempotency: BTreeMap<(String, String), (String, EdgeRoutePublicationResult)>,
    cutover_idempotency: BTreeMap<(String, String), (String, GatewayRouteCutoverResult)>,
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
        validate_gateway_certificate_transition(existing, &certificate, expected_version)?;
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
        let mut acknowledgement = acknowledgement.clone();
        acknowledgement.acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        let received_at = canonical_timestamp(received_at);
        acknowledgement
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if received_at < acknowledgement.acknowledged_at {
            return Err(RepositoryError::Conflict(
                "Gateway acknowledgement receipt predates its node timestamp".into(),
            ));
        }
        let node_id = NodeId::from_uuid(acknowledgement.node_id);
        let command_id = NodeCommandId::from_uuid(acknowledgement.command_id);
        let mut state = self.state.write().await;
        let Some(revision) = state.commands.get(&(node_id, command_id)).copied() else {
            return Ok(false);
        };
        let mut publication = state
            .publications
            .get(&(node_id, revision))
            .cloned()
            .ok_or_else(|| {
                RepositoryError::Storage(
                    "Gateway publication command references missing desired state".into(),
                )
            })?;
        let was_pending = publication.state == GatewayPublicationState::Pending;
        publication
            .acknowledge(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        if !was_pending {
            return Ok(true);
        }
        let certificate_ids = state
            .certificates
            .values()
            .filter(|certificate| {
                certificate.node_id == node_id
                    && certificate.gateway_revision == revision
                    && certificate.gateway_command_id == command_id
            })
            .map(|certificate| certificate.id)
            .collect::<Vec<_>>();
        if certificate_ids.len() != 1 {
            return Err(RepositoryError::Storage(
                "Gateway publication must have exactly one staged certificate".into(),
            ));
        }
        let certificate_id = certificate_ids[0];
        let mut certificate = state
            .certificates
            .get(&certificate_id)
            .cloned()
            .ok_or_else(|| RepositoryError::Storage("staged certificate disappeared".into()))?;
        certificate
            .apply_gateway_acknowledgement(&acknowledgement)
            .map_err(RepositoryError::Conflict)?;
        let route_ids = state
            .routes
            .values()
            .filter(|route| {
                route.gateway_node_id == node_id
                    && route.gateway_revision == Some(revision)
                    && route.gateway_command_id == Some(command_id)
            })
            .map(|route| route.id)
            .collect::<Vec<_>>();
        let cutover_id = state
            .cutovers
            .values()
            .find(|cutover| {
                cutover.node_id == node_id
                    && cutover.gateway_revision == revision
                    && cutover.gateway_command_id == command_id
            })
            .map(|cutover| cutover.deployment_id);
        if route_ids.is_empty() == cutover_id.is_none() {
            return Err(RepositoryError::Storage(
                "Gateway publication must select one route publication kind".into(),
            ));
        }
        if let Some(cutover_id) = cutover_id {
            let mut cutover = state
                .cutovers
                .get(&cutover_id)
                .cloned()
                .ok_or_else(|| RepositoryError::Storage("route cutover disappeared".into()))?;
            cutover
                .acknowledge(&acknowledgement)
                .map_err(RepositoryError::Conflict)?;
            if cutover.state == GatewayRouteCutoverState::Applied {
                validate_applied_cutover_routes(&state.routes, &cutover)?;
                for route in &cutover.routes {
                    state.routes.insert(route.id, route.clone());
                }
            }
            state.cutovers.insert(cutover_id, cutover);
        } else {
            for route_id in route_ids {
                state
                    .routes
                    .get_mut(&route_id)
                    .ok_or_else(|| RepositoryError::Storage("staged route disappeared".into()))?
                    .apply_gateway_acknowledgement(&acknowledgement)
                    .map_err(RepositoryError::Conflict)?;
            }
        }
        state.certificates.insert(certificate_id, certificate);
        state.publications.insert((node_id, revision), publication);
        if acknowledgement.state == GatewayAckState::Applied {
            let scope = state.scopes.get_mut(&node_id).ok_or_else(|| {
                RepositoryError::Storage("Gateway scope disappeared during acknowledgement".into())
            })?;
            scope.installed_revision = Some(revision);
            scope.aggregate_version += 1;
        }
        Ok(true)
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
                && route.workload_revision_id == cutover.previous_revision_id
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
            || current.workload_revision_id != cutover.previous_revision_id
            || current.gateway_node_id != cutover.node_id
            || candidate.state != RouteState::Publishing
            || candidate.workload_revision_id != cutover.candidate_revision_id
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
            || current.workload_revision_id != cutover.previous_revision_id
            || candidate.state != RouteState::Active
            || candidate.workload_revision_id != cutover.candidate_revision_id
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
        && current.gateway_node_id == candidate.gateway_node_id
        && current.hostname == candidate.hostname
        && current.path_prefix == candidate.path_prefix
        && current.domain_claim_id == candidate.domain_claim_id
        && current.domain_pattern == candidate.domain_pattern
        && current.workload_id == candidate.workload_id
        && current.port_name == candidate.port_name
        && current.created_at == candidate.created_at
}

fn validate_gateway_certificate_transition(
    existing: &GatewayCertificate,
    next: &GatewayCertificate,
    expected_version: u64,
) -> Result<(), RepositoryError> {
    use crate::modules::edge::domain::GatewayCertificateState;

    let transition_is_valid = match (existing.state, next.state) {
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Issued) => {
            next.csr_digest.is_some()
                && next.material.is_some()
                && next.failure.is_none()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Provisioning, GatewayCertificateState::Failed) => {
            next.csr_digest.is_some()
                && next.material.is_none()
                && next.failure.is_some()
                && next.ready_at.is_none()
                && next.revoked_at.is_none()
        }
        (GatewayCertificateState::Ready, GatewayCertificateState::Revoked) => {
            next.csr_digest == existing.csr_digest
                && next.material == existing.material
                && next.failure.is_some()
                && next.ready_at == existing.ready_at
                && next.revoked_at.is_some()
        }
        _ => false,
    };
    if existing.aggregate_version != expected_version
        || next.aggregate_version != expected_version.saturating_add(1)
        || !transition_is_valid
        || existing.id != next.id
        || existing.organization_id != next.organization_id
        || existing.node_id != next.node_id
        || existing.domain_claim_ids != next.domain_claim_ids
        || existing.gateway_revision != next.gateway_revision
        || existing.gateway_command_id != next.gateway_command_id
        || existing.snapshot_digest != next.snapshot_digest
        || existing.request != next.request
        || existing.created_at != next.created_at
        || next.updated_at < existing.updated_at
    {
        return Err(RepositoryError::Conflict(
            "Gateway certificate changed while applying its transition".into(),
        ));
    }
    Ok(())
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
