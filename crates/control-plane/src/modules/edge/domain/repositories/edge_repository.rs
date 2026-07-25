use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceState, GatewayCertificateState, GatewayPublication,
    GatewayPublicationState, GatewayReplicaRecoveryState, GatewayReplicaRolloutState,
    GatewayRollout, GatewayRolloutRollback, GatewayRolloutRollbackState, GatewayRolloutState,
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayScope, GatewayScopeState, Route,
    RouteState,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayRolloutId,
    GatewayScopeId, IdempotencyRequest, IdempotentWrite, NodeCommandId, NodeId, OrganizationId,
    ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, NodeGatewayAck, NodeGatewaySnapshotObservation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct StageRoutePublication {
    pub route: Route,
    pub gateway_scope: GatewayScope,
    pub certificate: GatewayCertificate,
    pub publication: GatewayPublication,
    pub expected_scope_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct StageGatewayRouteCutover {
    pub cutover: GatewayRouteCutover,
    pub certificate: GatewayCertificate,
    pub publication: GatewayPublication,
    pub expected_scope_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct StageGatewayCertificateConvergence {
    pub convergence: GatewayCertificateConvergence,
    pub certificate: Option<GatewayCertificate>,
    pub publication: GatewayPublication,
    pub expected_scope_version: u64,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct StageGatewayRollout {
    pub scope: GatewayScope,
    pub rollout: GatewayRollout,
    pub route_replicas: Vec<Route>,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub expected_scope_versions: std::collections::BTreeMap<NodeId, u64>,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
    pub route_event: Option<DomainEventEnvelope>,
}

#[derive(Debug, Clone)]
pub struct StageGatewayRolloutRollback {
    pub scope: GatewayScope,
    pub failed_rollout: GatewayRollout,
    pub rollback: GatewayRolloutRollback,
    pub rollout: GatewayRollout,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub reused_certificates: Vec<GatewayCertificate>,
    pub expected_scope_versions: std::collections::BTreeMap<NodeId, u64>,
    pub expected_rollback_version: u64,
    pub event: DomainEventEnvelope,
}

mod validation;

#[derive(Debug, Clone)]
pub struct CreateGatewayScopeWrite {
    pub scope: GatewayScope,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct CreateDomainClaimWrite {
    pub claim: DomainClaim,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct TransitionDomainClaim {
    pub claim: DomainClaim,
    pub expected_version: u64,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRoutePublicationResult {
    pub route: Route,
    pub certificate: GatewayCertificate,
    pub publication: GatewayPublication,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRouteCutoverResult {
    pub cutover: GatewayRouteCutover,
    pub certificate: GatewayCertificate,
    pub publication: GatewayPublication,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCertificateConvergenceResult {
    pub convergence: GatewayCertificateConvergence,
    pub certificate: Option<GatewayCertificate>,
    pub publication: GatewayPublication,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRolloutResult {
    pub rollout: GatewayRollout,
    #[serde(default)]
    pub route_replicas: Vec<Route>,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRolloutRollbackResult {
    pub rollback: GatewayRolloutRollback,
    pub rollout: GatewayRollout,
    pub publications: Vec<GatewayPublication>,
    pub certificates: Vec<GatewayCertificate>,
    pub reused_certificates: Vec<GatewayCertificate>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRolloutDispatchTarget {
    pub organization_id: OrganizationId,
    pub rollout: GatewayRollout,
    pub publications: Vec<GatewayPublication>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayReplicaRecoveryTarget {
    pub organization_id: OrganizationId,
    pub rollout: GatewayRollout,
    pub publication: GatewayPublication,
    pub prior_publication: Option<GatewayPublication>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRolloutRollbackTarget {
    pub scope: GatewayScope,
    pub failed_rollout: GatewayRollout,
    pub rollback: GatewayRolloutRollback,
}

impl GatewayRolloutRollbackTarget {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        self.failed_rollout.validate()?;
        self.rollback.validate()?;
        let failed_nodes = self
            .failed_rollout
            .replicas
            .iter()
            .map(|replica| replica.node_id)
            .collect::<std::collections::BTreeSet<_>>();
        let scope_nodes = self
            .scope
            .member_node_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if self.failed_rollout.id != self.rollback.failed_rollout_id
            || self.failed_rollout.gateway_scope_id != self.scope.id
            || self.failed_rollout.gateway_scope_id != self.rollback.gateway_scope_id
            || self.failed_rollout.membership_generation != self.scope.membership_generation
            || self.failed_rollout.membership_generation != self.rollback.membership_generation
            || self.failed_rollout.generation != self.rollback.failed_generation
            || self.failed_rollout.state != GatewayRolloutState::Degraded
            || self.failed_rollout.serves_traffic()?
            || self.failed_rollout.completed_at.is_none()
            || self.rollback.state != GatewayRolloutRollbackState::Required
            || failed_nodes != scope_nodes
            || self.failed_rollout.replicas.iter().any(|replica| {
                replica.state == GatewayReplicaRolloutState::Unavailable
                    && replica.recovery.as_ref().is_none_or(|recovery| {
                        recovery.state != GatewayReplicaRecoveryState::Observed
                    })
            })
        {
            return Err("Gateway rollout rollback target is not physically resolved".into());
        }
        Ok(())
    }
}

impl GatewayReplicaRecoveryTarget {
    pub fn validate(&self) -> Result<(), String> {
        self.rollout.validate()?;
        self.publication.snapshot()?;
        if self.organization_id.as_uuid().is_nil()
            || self.publication.command_correlation_id != self.rollout.correlation_id
            || self.publication.state != GatewayPublicationState::Unavailable
        {
            return Err("Gateway replica recovery target is invalid".into());
        }
        let replica = self
            .rollout
            .replicas
            .iter()
            .find(|replica| replica.node_id == self.publication.node_id)
            .ok_or_else(|| {
                "Gateway replica recovery publication does not belong to its rollout".to_string()
            })?;
        let recovery = replica
            .recovery
            .as_ref()
            .ok_or_else(|| "Gateway replica recovery target omitted recovery state".to_string())?;
        if replica.state != GatewayReplicaRolloutState::Unavailable
            || !matches!(
                recovery.state,
                GatewayReplicaRecoveryState::Required | GatewayReplicaRecoveryState::Observing
            )
            || replica.revision != self.publication.revision
            || replica.command_id != self.publication.command_id
            || replica.snapshot_digest != self.publication.snapshot_digest
            || replica.snapshot_expires_at != self.publication.snapshot_expires_at
            || replica.gateway_certificate_id
                != self
                    .publication
                    .certificate_request
                    .as_ref()
                    .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
        {
            return Err("Gateway replica recovery projection is inconsistent".into());
        }
        match (
            self.publication.expected_revision,
            self.prior_publication.as_ref(),
        ) {
            (None, None) => {}
            (Some(expected_revision), Some(prior))
                if prior.node_id == self.publication.node_id
                    && prior.revision == expected_revision
                    && prior.revision < self.publication.revision =>
            {
                prior.snapshot()?;
            }
            _ => return Err("Gateway replica recovery prior publication is inconsistent".into()),
        }
        Ok(())
    }
}

impl GatewayRolloutDispatchTarget {
    pub fn validate(&self) -> Result<(), String> {
        self.rollout.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || !matches!(
                self.rollout.state,
                GatewayRolloutState::Pending | GatewayRolloutState::Ready
            )
            || self
                .publications
                .windows(2)
                .any(|publications| publications[0].node_id >= publications[1].node_id)
        {
            return Err("Gateway rollout dispatch target is invalid".into());
        }
        let pending_replicas = self
            .rollout
            .replicas
            .iter()
            .filter(|replica| replica.state == GatewayReplicaRolloutState::Pending)
            .collect::<Vec<_>>();
        if pending_replicas.is_empty() || pending_replicas.len() != self.publications.len() {
            return Err(
                "Gateway rollout dispatch target does not cover every pending replica".into(),
            );
        }
        for (replica, publication) in pending_replicas.into_iter().zip(&self.publications) {
            publication.snapshot()?;
            if publication.node_id != replica.node_id
                || publication.revision != replica.revision
                || publication.command_id != replica.command_id
                || publication.command_correlation_id != self.rollout.correlation_id
                || publication.snapshot_digest != replica.snapshot_digest
                || publication.snapshot_expires_at != replica.snapshot_expires_at
                || publication
                    .certificate_request
                    .as_ref()
                    .map(|request| GatewayCertificateId::from_uuid(request.certificate_id))
                    != replica.gateway_certificate_id
                || publication.state != GatewayPublicationState::Pending
                || publication.failure.is_some()
                || publication.acknowledged_at.is_some()
                || publication.command_issued_at != self.rollout.started_at
            {
                return Err(
                    "Gateway rollout dispatch publication does not match its pending replica"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateRouteStatus {
    pub route: Route,
    pub domain_claim_state: DomainClaimState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateConvergenceTarget {
    pub scope: GatewayScopeState,
    pub publication: GatewayPublication,
    pub certificate: GatewayCertificate,
    pub routes: Vec<GatewayCertificateRouteStatus>,
}

impl GatewayCertificateConvergenceTarget {
    pub fn validate(&self) -> Result<(), String> {
        let installed_revision = self
            .scope
            .installed_revision
            .ok_or_else(|| "Gateway certificate convergence target is not installed".to_string())?;
        if self.routes.is_empty()
            || self.publication.node_id != self.scope.node_id
            || self.publication.revision != installed_revision
            || self.publication.state
                != crate::modules::edge::domain::GatewayPublicationState::Applied
            || self.publication.acknowledged_at.is_none()
            || self.certificate.node_id != self.scope.node_id
            || !matches!(
                self.certificate.state,
                crate::modules::edge::domain::GatewayCertificateState::Ready
                    | crate::modules::edge::domain::GatewayCertificateState::Revoked
            )
            || self.routes.iter().any(|status| {
                status.route.gateway_node_id != self.scope.node_id
                    || status.route.organization_id != self.certificate.organization_id
                    || status.route.state != RouteState::Active
                    || status.route.gateway_certificate_id != Some(self.certificate.id)
            })
        {
            return Err("Gateway certificate convergence target is inconsistent".into());
        }
        self.publication.snapshot()?;
        Ok(())
    }
}

#[async_trait]
pub trait IEdgeRepository: Send + Sync {
    async fn create_gateway_scope(
        &self,
        bundle: CreateGatewayScopeWrite,
    ) -> Result<IdempotentWrite<GatewayScope>, RepositoryError>;

    async fn find_gateway_scope(
        &self,
        organization_id: OrganizationId,
        scope_id: GatewayScopeId,
    ) -> Result<GatewayScope, RepositoryError>;

    async fn list_gateway_scopes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<GatewayScope>, RepositoryError>;

    async fn replay_domain_claim_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<DomainClaim>, RepositoryError>;

    async fn create_domain_claim(
        &self,
        bundle: CreateDomainClaimWrite,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError>;

    async fn transition_domain_claim(
        &self,
        bundle: TransitionDomainClaim,
    ) -> Result<IdempotentWrite<DomainClaim>, RepositoryError>;

    async fn find_domain_claim(
        &self,
        organization_id: OrganizationId,
        claim_id: DomainClaimId,
    ) -> Result<DomainClaim, RepositoryError>;

    async fn list_domain_claims(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<DomainClaim>, RepositoryError>;

    async fn replay_route_publication(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<EdgeRoutePublicationResult>, RepositoryError>;

    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError>;

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError>;

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError>;

    async fn replay_gateway_route_cutover(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<GatewayRouteCutoverResult>, RepositoryError>;

    async fn stage_gateway_route_cutover(
        &self,
        bundle: StageGatewayRouteCutover,
    ) -> Result<GatewayRouteCutoverResult, RepositoryError>;

    async fn stage_gateway_rollout(
        &self,
        bundle: StageGatewayRollout,
    ) -> Result<GatewayRolloutResult, RepositoryError>;

    async fn stage_gateway_rollout_rollback(
        &self,
        bundle: StageGatewayRolloutRollback,
    ) -> Result<GatewayRolloutRollbackResult, RepositoryError>;

    async fn replay_gateway_rollout(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<GatewayRolloutResult>, RepositoryError>;

    async fn next_gateway_rollout_generation(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
    ) -> Result<u64, RepositoryError>;

    async fn pending_gateway_rollout_dispatches(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayRolloutDispatchTarget>, RepositoryError>;

    async fn find_gateway_rollout(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRollout, RepositoryError>;

    async fn find_gateway_rollout_rollback(
        &self,
        organization_id: OrganizationId,
        failed_rollout_id: GatewayRolloutId,
    ) -> Result<GatewayRolloutRollback, RepositoryError>;

    async fn pending_gateway_rollout_rollbacks(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayRolloutRollbackTarget>, RepositoryError>;

    async fn mark_gateway_rollout_replica_unavailable(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError>;

    async fn pending_gateway_replica_recoveries(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayReplicaRecoveryTarget>, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn stage_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError>;

    async fn record_gateway_replica_recovery_observation(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        observation: NodeGatewaySnapshotObservation,
    ) -> Result<GatewayRollout, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn record_gateway_replica_recovery_failure(
        &self,
        organization_id: OrganizationId,
        rollout_id: GatewayRolloutId,
        node_id: NodeId,
        expected_version: u64,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
        failure: &str,
        retryable: bool,
        failed_at: DateTime<Utc>,
    ) -> Result<GatewayRollout, RepositoryError>;

    async fn gateway_certificate_convergence_targets(
        &self,
        certificate_renew_before: DateTime<Utc>,
        snapshot_renew_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError>;

    async fn pending_gateway_certificate_convergences(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError>;

    async fn stage_gateway_certificate_convergence(
        &self,
        bundle: StageGatewayCertificateConvergence,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn mark_gateway_certificate_convergence_unavailable(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<GatewayCertificateConvergenceResult, RepositoryError>;

    async fn find_gateway_certificate_convergence(
        &self,
        node_id: NodeId,
        gateway_revision: u64,
    ) -> Result<Option<GatewayCertificateConvergence>, RepositoryError>;

    async fn obsolete_gateway_certificates(
        &self,
        limit: usize,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError>;

    async fn find_gateway_route_cutover(
        &self,
        organization_id: OrganizationId,
        deployment_id: DeploymentId,
    ) -> Result<Option<GatewayRouteCutover>, RepositoryError>;

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError>;

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError>;

    async fn find_gateway_certificate(
        &self,
        node_id: NodeId,
        certificate_id: GatewayCertificateId,
    ) -> Result<GatewayCertificate, RepositoryError>;

    async fn list_gateway_certificates(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<GatewayCertificate>, RepositoryError>;

    async fn transition_gateway_certificate(
        &self,
        certificate: GatewayCertificate,
        expected_version: u64,
    ) -> Result<GatewayCertificate, RepositoryError>;

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError>;
}
