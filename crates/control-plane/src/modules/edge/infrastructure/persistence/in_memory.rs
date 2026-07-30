use crate::modules::edge::domain::repositories::{
    CreateDomainClaimWrite, CreateGatewayScopeWrite, EdgeRoutePublicationResult,
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayReplicaRecoveryTarget, GatewayRolloutDispatchTarget, GatewayRolloutResult,
    GatewayRolloutRollbackResult, GatewayRouteCutoverResult, IEdgeRepository,
    StageGatewayCertificateConvergence, StageGatewayRollout, StageGatewayRolloutRollback,
    StageGatewayRouteCutover, StageRoutePublication, TransitionDomainClaim,
};
use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayPublication, GatewayPublicationState, GatewayRollout, GatewayRolloutRollback,
    GatewayRouteCutover, GatewayScope, GatewayScopeState, McpCredential, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, IdempotencyRequest, IdempotentWrite, McpCredentialId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, NodeGatewayAck, NodeGatewaySnapshotObservation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;

mod acknowledgements;
mod certificate_convergence;
mod certificates;
mod gateway_scopes;
mod mcp_credentials;
mod rollouts;
mod validation;

use validation::{
    validate_applied_cutover_routes, validate_domain_event, validate_pending_cutover_routes,
};

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
    rollout_rollbacks: BTreeMap<GatewayRolloutId, GatewayRolloutRollback>,
    rollout_publications: BTreeMap<(NodeId, u64), GatewayRolloutId>,
    rollout_route_projections: BTreeMap<(GatewayRolloutId, NodeId), Route>,
    rollout_idempotency: BTreeMap<(String, String), (String, GatewayRolloutResult)>,
    mcp_credentials: BTreeMap<McpCredentialId, McpCredential>,
    mcp_credential_prefixes: BTreeMap<String, McpCredentialId>,
    outbox: Vec<DomainEventEnvelope>,
}

impl InMemoryEdgeRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn outbox_events(&self) -> Vec<DomainEventEnvelope> {
        self.state.read().await.outbox.clone()
    }

    #[cfg(test)]
    pub(super) async fn gateway_publication(
        &self,
        node_id: NodeId,
        revision: u64,
    ) -> Option<GatewayPublication> {
        self.state
            .read()
            .await
            .publications
            .get(&(node_id, revision))
            .cloned()
    }

    #[cfg(test)]
    pub(super) async fn gateway_route_projection(
        &self,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
    ) -> Option<Route> {
        self.state
            .read()
            .await
            .rollout_route_projections
            .get(&(rollout_id, node_id))
            .cloned()
    }

    #[cfg(test)]
    pub(super) async fn gateway_route_owner(
        &self,
        node_id: NodeId,
        hostname: &str,
        path_prefix: &str,
    ) -> Option<RouteId> {
        self.state
            .read()
            .await
            .ownership
            .get(&(node_id, hostname.into(), path_prefix.into()))
            .copied()
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
        let state = self.state.read().await;
        let rollout_route_ids = state
            .rollout_route_projections
            .values()
            .map(|route| route.id)
            .collect::<BTreeSet<_>>();
        let mut routes = state
            .routes
            .values()
            .filter(|route| {
                route.gateway_node_id == node_id
                    && route.state == RouteState::Active
                    && !rollout_route_ids.contains(&route.id)
            })
            .cloned()
            .collect::<Vec<_>>();
        routes.extend(
            state
                .rollout_route_projections
                .values()
                .filter(|projection| {
                    projection.gateway_node_id == node_id
                        && projection.state == RouteState::Active
                        && state
                            .routes
                            .get(&projection.id)
                            .is_some_and(|logical| logical.state == RouteState::Active)
                })
                .cloned(),
        );
        routes.sort_by(|left, right| {
            (left.hostname.as_str(), left.path_prefix.as_str(), left.id).cmp(&(
                right.hostname.as_str(),
                right.path_prefix.as_str(),
                right.id,
            ))
        });
        Ok(routes)
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

    async fn stage_gateway_rollout_rollback(
        &self,
        bundle: StageGatewayRolloutRollback,
    ) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::stage_rollback(&mut state, bundle)
    }

    async fn replay_gateway_rollout(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<GatewayRolloutResult>, RepositoryError> {
        let state = self.state.read().await;
        rollouts::replay(&state, idempotency)
    }

    async fn next_gateway_rollout_generation(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
    ) -> Result<u64, RepositoryError> {
        let state = self.state.read().await;
        rollouts::next_generation(&state, organization_id, gateway_scope_id)
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

    async fn find_gateway_rollout_rollback(
        &self,
        organization_id: OrganizationId,
        failed_rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRolloutRollback, RepositoryError> {
        let state = self.state.read().await;
        rollouts::find_rollback(&state, organization_id, failed_rollout_id)
    }

    async fn pending_gateway_rollout_rollbacks(
        &self,
        limit: usize,
    ) -> Result<
        Vec<crate::modules::edge::domain::repositories::GatewayRolloutRollbackTarget>,
        RepositoryError,
    > {
        let state = self.state.read().await;
        rollouts::pending_rollbacks(&state, limit)
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

    async fn pending_gateway_replica_recoveries(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayReplicaRecoveryTarget>, RepositoryError> {
        let state = self.state.read().await;
        rollouts::pending_recoveries(&state, limit)
    }

    async fn stage_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: NodeCommandId,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::stage_recovery_observation(
            &mut state,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            command_id,
            issued_at,
            not_after,
        )
    }

    async fn record_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        observation: NodeGatewaySnapshotObservation,
    ) -> Result<GatewayRollout, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::record_recovery_observation(
            &mut state,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            observation,
        )
    }

    async fn record_gateway_replica_recovery_failure(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: NodeCommandId,
        failure: &str,
        retryable: bool,
        failed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError> {
        let mut state = self.state.write().await;
        rollouts::record_recovery_failure(
            &mut state,
            organization_id,
            rollout_id,
            node_id,
            expected_version,
            command_id,
            failure,
            retryable,
            failed_at,
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

    async fn mark_gateway_certificate_convergence_unavailable(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
        let mut state = self.state.write().await;
        certificate_convergence::mark_unavailable(
            &mut state,
            organization_id,
            node_id,
            gateway_revision,
            gateway_command_id,
            failure,
            observed_at,
        )
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
