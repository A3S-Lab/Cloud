use crate::modules::edge::domain::events::McpGatewaySnapshotStaged;
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublication, GatewayPublicationState,
};
use crate::modules::edge::infrastructure::CompiledMcpGatewaySnapshot;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StageMcpGatewaySnapshot {
    candidate: CompiledMcpGatewaySnapshot,
    publication: GatewayPublication,
    certificate: Option<GatewayCertificate>,
    event: DomainEventEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotStageResult {
    pub publication: GatewayPublication,
    pub certificate: Option<GatewayCertificate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotDispatchTarget {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub publication: GatewayPublication,
}

impl McpGatewaySnapshotDispatchTarget {
    pub fn validate(&self) -> Result<(), String> {
        self.publication.snapshot()?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.publication.state != GatewayPublicationState::Pending
            || self.publication.failure.is_some()
            || self.publication.acknowledged_at.is_some()
        {
            return Err("MCP Gateway snapshot dispatch target is inconsistent".into());
        }
        Ok(())
    }
}

impl StageMcpGatewaySnapshot {
    pub fn new(
        candidate: CompiledMcpGatewaySnapshot,
        command_id: NodeCommandId,
        command_correlation_id: Uuid,
        command_not_after: DateTime<Utc>,
    ) -> Result<Self, String> {
        if command_id.as_uuid().is_nil() {
            return Err("MCP Gateway snapshot command ID must not be nil".into());
        }
        let snapshot = candidate.snapshot().clone();
        let publication = GatewayPublication::stage(
            candidate.physical_scope().node_id,
            command_id,
            command_correlation_id,
            snapshot,
            candidate.snapshot().issued_at,
            command_not_after,
        )?;
        let domain_claim_ids = domain_claim_ids(&candidate);
        let certificate = publication
            .certificate_request
            .clone()
            .map(|request| {
                GatewayCertificate::provision(
                    GatewayCertificateId::from_uuid(request.certificate_id),
                    candidate.mcp().scope().organization_id,
                    publication.node_id,
                    domain_claim_ids.clone(),
                    publication.revision,
                    publication.command_id,
                    publication.snapshot_digest.clone(),
                    request,
                    publication.command_issued_at,
                )
            })
            .transpose()?;
        let next_physical_scope_version = candidate
            .physical_scope()
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway scope aggregate version space is exhausted".to_string())?;
        let event = McpGatewaySnapshotStaged::envelope(
            candidate.mcp().scope(),
            next_physical_scope_version,
            &publication,
            ordinary_route_ids(&candidate),
            mcp_route_ids(&candidate),
            domain_claim_ids,
        )?;
        let stage = Self {
            candidate,
            publication,
            certificate,
            event,
        };
        stage.validate()?;
        Ok(stage)
    }

    pub const fn candidate(&self) -> &CompiledMcpGatewaySnapshot {
        &self.candidate
    }

    pub const fn publication(&self) -> &GatewayPublication {
        &self.publication
    }

    pub const fn certificate(&self) -> Option<&GatewayCertificate> {
        self.certificate.as_ref()
    }

    pub const fn event(&self) -> &DomainEventEnvelope {
        &self.event
    }

    pub fn validate(&self) -> Result<(), String> {
        let snapshot = self.publication.snapshot()?;
        let expected_scope_version = self
            .candidate
            .physical_scope()
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway scope aggregate version space is exhausted".to_string())?;
        let payload =
            serde_json::from_value::<McpGatewaySnapshotStaged>(self.event.payload.clone())
                .map_err(|error| error.to_string())?;
        let ordinary_route_ids = ordinary_route_ids(&self.candidate);
        let mcp_route_ids = mcp_route_ids(&self.candidate);
        let domain_claim_ids = domain_claim_ids(&self.candidate);
        if snapshot != *self.candidate.snapshot()
            || self.publication.state != GatewayPublicationState::Pending
            || self.publication.failure.is_some()
            || self.publication.acknowledged_at.is_some()
            || self.publication.node_id != self.candidate.physical_scope().node_id
            || self.publication.command_issued_at != self.candidate.mcp().observed_at()
            || self.event.event_key != "edge.mcp-gateway.snapshot-staged"
            || self.event.schema_version != 1
            || self.event.organization_id != self.candidate.mcp().scope().organization_id.as_uuid()
            || self.event.aggregate_id != self.publication.node_id.as_uuid()
            || self.event.aggregate_version != expected_scope_version
            || self.event.occurred_at != self.publication.command_issued_at
            || self.event.correlation_id != self.publication.command_correlation_id
            || payload.organization_id != self.candidate.mcp().scope().organization_id
            || payload.project_id != self.candidate.mcp().scope().project_id
            || payload.environment_id != self.candidate.mcp().scope().environment_id
            || payload.gateway_scope_id != self.candidate.mcp().scope().id
            || payload.node_id != self.publication.node_id
            || payload.gateway_revision != self.publication.revision
            || payload.gateway_command_id != self.publication.command_id
            || payload.snapshot_digest != self.publication.snapshot_digest
            || payload.ordinary_route_ids != ordinary_route_ids
            || payload.mcp_route_ids != mcp_route_ids
            || payload.domain_claim_ids != domain_claim_ids
        {
            return Err("MCP Gateway snapshot stage bundle is inconsistent".into());
        }
        match (
            self.publication.certificate_request.as_ref(),
            self.certificate.as_ref(),
        ) {
            (Some(request), Some(certificate))
                if certificate.id.as_uuid() == request.certificate_id
                    && certificate.organization_id
                        == self.candidate.mcp().scope().organization_id
                    && certificate.node_id == self.publication.node_id
                    && certificate.domain_claim_ids == domain_claim_ids
                    && certificate.gateway_revision == self.publication.revision
                    && certificate.gateway_command_id == self.publication.command_id
                    && certificate.snapshot_digest == self.publication.snapshot_digest
                    && certificate.request == *request
                    && certificate.state == GatewayCertificateState::Provisioning
                    && certificate.csr_digest.is_none()
                    && certificate.material.is_none()
                    && certificate.failure.is_none()
                    && certificate.aggregate_version == 1
                    && payload.gateway_certificate_id == Some(certificate.id) => {}
            (None, None)
                if domain_claim_ids.is_empty() && payload.gateway_certificate_id.is_none() => {}
            _ => {
                return Err(
                    "MCP Gateway snapshot certificate staging evidence is inconsistent".into(),
                )
            }
        }
        Ok(())
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        CompiledMcpGatewaySnapshot,
        GatewayPublication,
        Option<GatewayCertificate>,
        DomainEventEnvelope,
    ) {
        (
            self.candidate,
            self.publication,
            self.certificate,
            self.event,
        )
    }
}

#[async_trait]
pub trait IMcpGatewaySnapshotRepository: Send + Sync {
    async fn stage_mcp_gateway_snapshot(
        &self,
        stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError>;

    async fn pending_mcp_gateway_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn mark_mcp_gateway_snapshot_unavailable(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError>;
}

fn ordinary_route_ids(candidate: &CompiledMcpGatewaySnapshot) -> Vec<RouteId> {
    candidate
        .active_route_versions()
        .iter()
        .map(|version| version.route_id)
        .collect()
}

fn mcp_route_ids(candidate: &CompiledMcpGatewaySnapshot) -> Vec<RouteId> {
    candidate
        .mcp()
        .route_versions()
        .iter()
        .map(|version| version.route_id())
        .collect()
}

fn domain_claim_ids(candidate: &CompiledMcpGatewaySnapshot) -> Vec<DomainClaimId> {
    candidate
        .domain_claim_versions()
        .iter()
        .map(|version| version.domain_claim_id())
        .collect()
}
