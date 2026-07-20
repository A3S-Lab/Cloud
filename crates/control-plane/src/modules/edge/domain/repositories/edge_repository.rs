use crate::modules::edge::domain::{
    DomainClaim, DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceState, GatewayPublication, GatewayRouteCutover,
    GatewayRouteCutoverState, GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, DomainClaimId, EnvironmentId, GatewayCertificateId, IdempotencyRequest,
    IdempotentWrite, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::{DomainEventEnvelope, NodeGatewayAck};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct StageRoutePublication {
    pub route: Route,
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

impl StageGatewayCertificateConvergence {
    pub fn validate(&self) -> Result<(), String> {
        self.convergence.validate()?;
        let convergence = &self.convergence;
        let publication = &self.publication;
        if convergence.state != GatewayCertificateConvergenceState::Pending
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || convergence.node_id != publication.node_id
            || convergence.gateway_revision != publication.revision
            || convergence.gateway_command_id != publication.command_id
            || convergence.snapshot_digest != publication.snapshot_digest
            || publication.expected_revision.is_none()
            || self.event.organization_id != convergence.organization_id.as_uuid()
            || self.event.aggregate_id
                != convergence
                    .replacement_certificate_id
                    .unwrap_or(convergence.previous_certificate_id)
                    .as_uuid()
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err(
                "Gateway certificate convergence and complete publication are inconsistent".into(),
            );
        }
        match (
            convergence.replacement_certificate_id,
            publication.certificate_request.as_ref(),
            self.certificate.as_ref(),
        ) {
            (Some(certificate_id), Some(request), Some(certificate))
                if request.certificate_id == certificate_id.as_uuid()
                    && certificate.id == certificate_id
                    && certificate.organization_id == convergence.organization_id
                    && certificate.node_id == convergence.node_id
                    && certificate.gateway_revision == convergence.gateway_revision
                    && certificate.gateway_command_id == convergence.gateway_command_id
                    && certificate.snapshot_digest == convergence.snapshot_digest
                    && certificate.request == *request
                    && certificate.state
                        == crate::modules::edge::domain::GatewayCertificateState::Provisioning
                    && certificate.csr_digest.is_none()
                    && certificate.material.is_none() => {}
            (None, None, None) => {}
            _ => {
                return Err(
                    "Gateway certificate convergence replacement material is inconsistent".into(),
                )
            }
        }
        publication.snapshot()?;
        Ok(())
    }
}

impl StageGatewayRouteCutover {
    pub fn validate(&self) -> Result<(), String> {
        self.cutover.validate()?;
        let cutover = &self.cutover;
        let certificate = &self.certificate;
        let publication = &self.publication;
        if cutover.state != GatewayRouteCutoverState::Pending
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || cutover.node_id != publication.node_id
            || cutover.gateway_revision != publication.revision
            || cutover.gateway_command_id != publication.command_id
            || cutover.snapshot_digest != publication.snapshot_digest
            || cutover.gateway_certificate_id != certificate.id
            || certificate.organization_id != cutover.organization_id
            || certificate.node_id != cutover.node_id
            || certificate.gateway_revision != cutover.gateway_revision
            || certificate.gateway_command_id != cutover.gateway_command_id
            || certificate.snapshot_digest != cutover.snapshot_digest
            || publication.certificate_request.as_ref() != Some(&certificate.request)
            || certificate.state
                != crate::modules::edge::domain::GatewayCertificateState::Provisioning
            || certificate.csr_digest.is_some()
            || certificate.material.is_some()
            || self.event.organization_id != cutover.organization_id.as_uuid()
            || self.event.aggregate_id != cutover.deployment_id.as_uuid()
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err("route cutover and complete Gateway publication are inconsistent".into());
        }
        publication.snapshot()?;
        Ok(())
    }
}

impl StageRoutePublication {
    pub fn validate(&self) -> Result<(), String> {
        let route = &self.route;
        let certificate = &self.certificate;
        let publication = &self.publication;
        if route.state != crate::modules::edge::domain::RouteState::Publishing
            || route.gateway_node_id != publication.node_id
            || route.gateway_revision != Some(publication.revision)
            || route.gateway_command_id != Some(publication.command_id)
            || route.snapshot_digest.as_deref() != Some(&publication.snapshot_digest)
            || publication.state != crate::modules::edge::domain::GatewayPublicationState::Pending
            || route.gateway_certificate_id != Some(certificate.id)
            || certificate.node_id != publication.node_id
            || certificate.gateway_revision != publication.revision
            || certificate.gateway_command_id != publication.command_id
            || certificate.snapshot_digest != publication.snapshot_digest
            || publication.certificate_request.as_ref() != Some(&certificate.request)
            || certificate.state
                != crate::modules::edge::domain::GatewayCertificateState::Provisioning
            || certificate.csr_digest.is_some()
            || certificate.material.is_some()
            || route
                .domain_claim_id
                .is_none_or(|claim_id| !certificate.domain_claim_ids.contains(&claim_id))
            || self.event.correlation_id != publication.command_correlation_id
        {
            return Err("route and complete Gateway publication are inconsistent".into());
        }
        publication.snapshot()?;
        Ok(())
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateRouteStatus {
    pub route: Route,
    pub domain_claim_state: DomainClaimState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayCertificateConvergenceTarget {
    pub scope: GatewayScopeState,
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
            || self.certificate.node_id != self.scope.node_id
            || self.certificate.gateway_revision != installed_revision
            || !matches!(
                self.certificate.state,
                crate::modules::edge::domain::GatewayCertificateState::Ready
                    | crate::modules::edge::domain::GatewayCertificateState::Revoked
            )
            || self.routes.iter().any(|status| {
                status.route.gateway_node_id != self.scope.node_id
                    || status.route.organization_id != self.certificate.organization_id
                    || status.route.state != RouteState::Active
            })
        {
            return Err("Gateway certificate convergence target is inconsistent".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IEdgeRepository: Send + Sync {
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

    async fn gateway_certificate_convergence_targets(
        &self,
        renew_before: DateTime<Utc>,
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
