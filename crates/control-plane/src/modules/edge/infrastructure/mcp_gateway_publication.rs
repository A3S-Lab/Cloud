use crate::modules::edge::domain::events::McpGatewaySnapshotStaged;
use crate::modules::edge::domain::repositories::{
    EdgeRoutePublicationResult, GatewayRolloutResult, GatewayRolloutRollbackResult,
    GatewayRouteCutoverResult, StageGatewayRollout, StageGatewayRolloutRollback,
    StageGatewayRouteCutover, StageRoutePublication,
};
use crate::modules::edge::domain::{
    GatewayCertificate, GatewayCertificateState, GatewayPublication, GatewayPublicationState,
    GatewayScope, GatewayScopeState,
};
use crate::modules::edge::infrastructure::CompiledMcpGatewaySnapshot;
use crate::modules::edge::infrastructure::GatewaySnapshotRouteInput;
use crate::modules::edge::infrastructure::McpGatewaySnapshotAnchor;
use crate::modules::shared_kernel::domain::{
    DomainClaimId, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeCommandId, NodeId,
    OrganizationId, ProjectId, RepositoryError, RouteId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StageMcpGatewaySnapshot {
    composition: GatewayManagedSnapshotComposition,
    publication: GatewayPublication,
    certificate: Option<GatewayCertificate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewaySnapshotPublicationOwner {
    McpReconciler,
    Ordinary,
}

impl GatewaySnapshotPublicationOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::McpReconciler => "mcp-reconciler",
            Self::Ordinary => "ordinary",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mcp-reconciler" => Ok(Self::McpReconciler),
            "ordinary" => Ok(Self::Ordinary),
            _ => Err("Gateway snapshot publication owner is invalid".into()),
        }
    }
}

/// Immutable MCP desired-state evidence attached to any complete Gateway
/// publication, including publications initiated by ordinary Route flows.
#[derive(Debug, Clone)]
pub struct GatewayManagedSnapshotComposition {
    candidate: CompiledMcpGatewaySnapshot,
    owner: GatewaySnapshotPublicationOwner,
    event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct StageManagedRoutePublication {
    ordinary: StageRoutePublication,
    composition: GatewayManagedSnapshotComposition,
}

#[derive(Debug, Clone)]
pub struct StageManagedGatewayRouteCutover {
    ordinary: StageGatewayRouteCutover,
    composition: GatewayManagedSnapshotComposition,
}

#[derive(Debug, Clone)]
pub struct StageManagedGatewayRollout {
    ordinary: StageGatewayRollout,
    compositions: BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
}

#[derive(Debug, Clone)]
pub struct StageManagedGatewayRolloutRollback {
    ordinary: StageGatewayRolloutRollback,
    compositions: BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotStatus {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub desired_state_digest: crate::modules::shared_kernel::domain::Sha256Digest,
    pub desired_gateway_scope_ids: Vec<GatewayScopeId>,
    pub mcp_route_count: u32,
    pub publication: GatewayPublication,
}

impl McpGatewaySnapshotStatus {
    pub const fn anchor(&self) -> McpGatewaySnapshotAnchor {
        McpGatewaySnapshotAnchor {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            gateway_scope_id: self.gateway_scope_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.publication.snapshot()?;
        self.anchor().validate()?;
        if self
            .desired_gateway_scope_ids
            .iter()
            .any(|scope_id| scope_id.as_uuid().is_nil())
            || self
                .desired_gateway_scope_ids
                .windows(2)
                .any(|scope_ids| scope_ids[0] >= scope_ids[1])
            || self
                .desired_gateway_scope_ids
                .first()
                .is_some_and(|scope_id| *scope_id != self.gateway_scope_id)
            || self.desired_gateway_scope_ids.len() > 1_000
            || self.mcp_route_count > 0 && self.desired_gateway_scope_ids.is_empty()
            || self.mcp_route_count > 1_000
        {
            return Err("MCP Gateway snapshot status is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewayReconciliationScope {
    pub scope: GatewayScope,
    pub node_ids: Vec<NodeId>,
}

impl McpGatewayReconciliationScope {
    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        if self.node_ids.is_empty()
            || self.node_ids.len() > 10_000
            || self
                .node_ids
                .iter()
                .any(|node_id| node_id.as_uuid().is_nil())
            || self
                .node_ids
                .windows(2)
                .any(|node_ids| node_ids[0] >= node_ids[1])
        {
            return Err("MCP Gateway reconciliation scope targets are invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotReconciliationState {
    pub pending_publication: bool,
    pub latest_mcp_snapshot: Option<McpGatewaySnapshotStatus>,
}

impl McpGatewaySnapshotReconciliationState {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(status) = &self.latest_mcp_snapshot {
            status.validate()?;
            if status.publication.state == GatewayPublicationState::Pending
                && !self.pending_publication
            {
                return Err(
                    "pending MCP Gateway snapshot is missing physical pending-publication evidence"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGatewaySnapshotInputs {
    pub physical_scope: GatewayScopeState,
    pub active_routes: Vec<GatewaySnapshotRouteInput>,
}

impl McpGatewaySnapshotInputs {
    pub fn validate(&self, node_id: NodeId) -> Result<(), String> {
        if self.physical_scope.node_id != node_id
            || self
                .active_routes
                .iter()
                .any(|input| input.route.gateway_node_id != node_id)
        {
            return Err(
                "MCP Gateway snapshot reconciliation inputs crossed a physical node".into(),
            );
        }
        Ok(())
    }
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
                    candidate.mcp().organization_id(),
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
        let composition = GatewayManagedSnapshotComposition::new(
            candidate,
            &publication,
            GatewaySnapshotPublicationOwner::McpReconciler,
        )?;
        let stage = Self {
            composition,
            publication,
            certificate,
        };
        stage.validate()?;
        Ok(stage)
    }

    pub const fn candidate(&self) -> &CompiledMcpGatewaySnapshot {
        self.composition.candidate()
    }

    pub const fn composition(&self) -> &GatewayManagedSnapshotComposition {
        &self.composition
    }

    pub const fn publication(&self) -> &GatewayPublication {
        &self.publication
    }

    pub const fn certificate(&self) -> Option<&GatewayCertificate> {
        self.certificate.as_ref()
    }

    pub const fn event(&self) -> &DomainEventEnvelope {
        self.composition.event()
    }

    pub fn validate(&self) -> Result<(), String> {
        let snapshot = self.publication.snapshot()?;
        self.composition.validate_for(&self.publication)?;
        let candidate = self.composition.candidate();
        let payload = serde_json::from_value::<McpGatewaySnapshotStaged>(
            self.composition.event().payload.clone(),
        )
        .map_err(|error| error.to_string())?;
        let domain_claim_ids = domain_claim_ids(candidate);
        if snapshot != *candidate.snapshot()
            || self.publication.state != GatewayPublicationState::Pending
            || self.publication.failure.is_some()
            || self.publication.acknowledged_at.is_some()
            || self.publication.node_id != candidate.physical_scope().node_id
            || self.publication.command_issued_at != candidate.mcp().observed_at()
        {
            return Err("MCP Gateway snapshot stage bundle is inconsistent".into());
        }
        match (
            self.publication.certificate_request.as_ref(),
            self.certificate.as_ref(),
        ) {
            (Some(request), Some(certificate))
                if certificate.id.as_uuid() == request.certificate_id
                    && certificate.organization_id == candidate.mcp().organization_id()
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
        GatewayManagedSnapshotComposition,
        GatewayPublication,
        Option<GatewayCertificate>,
    ) {
        (self.composition, self.publication, self.certificate)
    }
}

impl GatewayManagedSnapshotComposition {
    pub fn new(
        candidate: CompiledMcpGatewaySnapshot,
        publication: &GatewayPublication,
        owner: GatewaySnapshotPublicationOwner,
    ) -> Result<Self, String> {
        let next_physical_scope_version = candidate
            .physical_scope()
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway scope aggregate version space is exhausted".to_string())?;
        let event = McpGatewaySnapshotStaged::envelope(
            candidate.mcp().anchor().organization_id,
            candidate.mcp().anchor().project_id,
            candidate.mcp().anchor().environment_id,
            candidate.mcp().anchor().gateway_scope_id,
            candidate.mcp().scope_ids().to_vec(),
            next_physical_scope_version,
            publication,
            ordinary_route_ids(&candidate),
            mcp_route_ids(&candidate),
            domain_claim_ids(&candidate),
        )?;
        let composition = Self {
            candidate,
            owner,
            event,
        };
        composition.validate_for(publication)?;
        Ok(composition)
    }

    pub const fn candidate(&self) -> &CompiledMcpGatewaySnapshot {
        &self.candidate
    }

    pub const fn owner(&self) -> GatewaySnapshotPublicationOwner {
        self.owner
    }

    pub const fn event(&self) -> &DomainEventEnvelope {
        &self.event
    }

    pub fn validate_for(&self, publication: &GatewayPublication) -> Result<(), String> {
        publication.snapshot()?;
        let expected_scope_version = self
            .candidate
            .physical_scope()
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway scope aggregate version space is exhausted".to_string())?;
        let payload =
            serde_json::from_value::<McpGatewaySnapshotStaged>(self.event.payload.clone())
                .map_err(|error| error.to_string())?;
        if publication.snapshot()? != *self.candidate.snapshot()
            || publication.node_id != self.candidate.physical_scope().node_id
            || publication.command_issued_at != self.candidate.mcp().observed_at()
            || self.event.event_key != "edge.mcp-gateway.snapshot-staged"
            || self.event.schema_version != 2
            || self.event.organization_id != self.candidate.mcp().organization_id().as_uuid()
            || self.event.aggregate_id != publication.node_id.as_uuid()
            || self.event.aggregate_version != expected_scope_version
            || self.event.occurred_at != publication.command_issued_at
            || self.event.correlation_id != publication.command_correlation_id
            || payload.organization_id != self.candidate.mcp().anchor().organization_id
            || payload.project_id != self.candidate.mcp().anchor().project_id
            || payload.environment_id != self.candidate.mcp().anchor().environment_id
            || payload.gateway_scope_id != self.candidate.mcp().anchor().gateway_scope_id
            || payload.desired_gateway_scope_ids != self.candidate.mcp().scope_ids()
            || payload.node_id != publication.node_id
            || payload.gateway_revision != publication.revision
            || payload.gateway_command_id != publication.command_id
            || payload.snapshot_digest != publication.snapshot_digest
            || payload.ordinary_route_ids != ordinary_route_ids(&self.candidate)
            || payload.mcp_route_ids != mcp_route_ids(&self.candidate)
            || payload.domain_claim_ids != domain_claim_ids(&self.candidate)
        {
            return Err("managed Gateway snapshot composition is inconsistent".into());
        }
        Ok(())
    }
}

impl StageManagedRoutePublication {
    pub fn new(
        ordinary: StageRoutePublication,
        composition: GatewayManagedSnapshotComposition,
    ) -> Result<Self, String> {
        ordinary.validate()?;
        composition.validate_for(&ordinary.publication)?;
        if composition.owner() != GatewaySnapshotPublicationOwner::Ordinary
            || composition.candidate().physical_scope().node_id != ordinary.publication.node_id
            || composition.candidate().mcp().organization_id() != ordinary.route.organization_id
            || ordinary.certificate.domain_claim_ids != domain_claim_ids(composition.candidate())
        {
            return Err("managed Route publication composition is inconsistent".into());
        }
        Ok(Self {
            ordinary,
            composition,
        })
    }

    pub const fn ordinary(&self) -> &StageRoutePublication {
        &self.ordinary
    }

    pub const fn composition(&self) -> &GatewayManagedSnapshotComposition {
        &self.composition
    }

    pub(crate) fn into_parts(self) -> (StageRoutePublication, GatewayManagedSnapshotComposition) {
        (self.ordinary, self.composition)
    }
}

impl StageManagedGatewayRouteCutover {
    pub fn new(
        ordinary: StageGatewayRouteCutover,
        composition: GatewayManagedSnapshotComposition,
    ) -> Result<Self, String> {
        ordinary.validate()?;
        composition.validate_for(&ordinary.publication)?;
        let ordinary_route_ids = composition
            .candidate()
            .ordinary_route_ids()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if composition.owner() != GatewaySnapshotPublicationOwner::Ordinary
            || composition.candidate().physical_scope().node_id != ordinary.publication.node_id
            || composition.candidate().mcp().organization_id() != ordinary.cutover.organization_id
            || ordinary.certificate.domain_claim_ids != domain_claim_ids(composition.candidate())
            || ordinary
                .cutover
                .routes
                .iter()
                .any(|route| !ordinary_route_ids.contains(&route.id))
        {
            return Err("managed Gateway Route cutover composition is inconsistent".into());
        }
        Ok(Self {
            ordinary,
            composition,
        })
    }

    pub const fn ordinary(&self) -> &StageGatewayRouteCutover {
        &self.ordinary
    }

    pub const fn composition(&self) -> &GatewayManagedSnapshotComposition {
        &self.composition
    }

    pub(crate) fn into_parts(
        self,
    ) -> (StageGatewayRouteCutover, GatewayManagedSnapshotComposition) {
        (self.ordinary, self.composition)
    }
}

impl StageManagedGatewayRollout {
    pub fn new(
        ordinary: StageGatewayRollout,
        compositions: BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
    ) -> Result<Self, String> {
        ordinary.validate()?;
        let publication_nodes = ordinary
            .publications
            .iter()
            .map(|publication| publication.node_id)
            .collect::<BTreeSet<_>>();
        if compositions.keys().copied().collect::<BTreeSet<_>>() != publication_nodes {
            return Err(
                "managed Gateway rollout compositions do not cover exact physical membership"
                    .into(),
            );
        }
        for publication in &ordinary.publications {
            let composition = compositions.get(&publication.node_id).ok_or_else(|| {
                "managed Gateway rollout publication omitted its composition".to_string()
            })?;
            composition.validate_for(publication)?;
            let certificate = ordinary
                .certificates
                .iter()
                .find(|certificate| certificate.node_id == publication.node_id)
                .ok_or_else(|| {
                    "managed Gateway rollout publication omitted its certificate".to_string()
                })?;
            if composition.owner() != GatewaySnapshotPublicationOwner::Ordinary
                || composition.candidate().mcp().organization_id() != ordinary.scope.organization_id
                || certificate.domain_claim_ids != domain_claim_ids(composition.candidate())
            {
                return Err("managed Gateway rollout composition is inconsistent".into());
            }
        }
        Ok(Self {
            ordinary,
            compositions,
        })
    }

    pub const fn ordinary(&self) -> &StageGatewayRollout {
        &self.ordinary
    }

    pub fn compositions(&self) -> &BTreeMap<NodeId, GatewayManagedSnapshotComposition> {
        &self.compositions
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        StageGatewayRollout,
        BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
    ) {
        (self.ordinary, self.compositions)
    }
}

impl StageManagedGatewayRolloutRollback {
    pub fn new(
        ordinary: StageGatewayRolloutRollback,
        compositions: BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
    ) -> Result<Self, String> {
        ordinary.validate()?;
        let publication_nodes = ordinary
            .publications
            .iter()
            .map(|publication| publication.node_id)
            .collect::<BTreeSet<_>>();
        if compositions.keys().copied().collect::<BTreeSet<_>>() != publication_nodes {
            return Err(
                "managed Gateway rollback compositions do not cover exact physical membership"
                    .into(),
            );
        }
        for publication in &ordinary.publications {
            let composition = compositions.get(&publication.node_id).ok_or_else(|| {
                "managed Gateway rollback publication omitted its composition".to_string()
            })?;
            composition.validate_for(publication)?;
            if composition.owner() != GatewaySnapshotPublicationOwner::Ordinary
                || composition.candidate().mcp().organization_id() != ordinary.scope.organization_id
            {
                return Err("managed Gateway rollback composition is inconsistent".into());
            }
            let expected_claims = domain_claim_ids(composition.candidate())
                .into_iter()
                .collect::<BTreeSet<_>>();
            match &publication.certificate_request {
                Some(request) => {
                    let certificate_id =
                        crate::modules::shared_kernel::domain::GatewayCertificateId::from_uuid(
                            request.certificate_id,
                        );
                    let replacement = ordinary
                        .certificates
                        .iter()
                        .find(|certificate| certificate.id == certificate_id);
                    let reused = ordinary
                        .reused_certificates
                        .iter()
                        .find(|certificate| certificate.id == certificate_id);
                    match (replacement, reused) {
                        (Some(certificate), None)
                            if certificate.node_id == publication.node_id
                                && certificate
                                    .domain_claim_ids
                                    .iter()
                                    .copied()
                                    .collect::<BTreeSet<_>>()
                                    == expected_claims => {}
                        (None, Some(certificate))
                            if certificate.node_id == publication.node_id
                                && expected_claims.is_subset(
                                    &certificate
                                        .domain_claim_ids
                                        .iter()
                                        .copied()
                                        .collect::<BTreeSet<_>>(),
                                ) => {}
                        _ => {
                            return Err(
                                "managed Gateway rollback certificate authority is incomplete"
                                    .into(),
                            )
                        }
                    }
                }
                None if expected_claims.is_empty() => {}
                None => {
                    return Err(
                        "managed Gateway rollback omitted its complete certificate authority"
                            .into(),
                    )
                }
            }
        }
        Ok(Self {
            ordinary,
            compositions,
        })
    }

    pub const fn ordinary(&self) -> &StageGatewayRolloutRollback {
        &self.ordinary
    }

    pub fn compositions(&self) -> &BTreeMap<NodeId, GatewayManagedSnapshotComposition> {
        &self.compositions
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        StageGatewayRolloutRollback,
        BTreeMap<NodeId, GatewayManagedSnapshotComposition>,
    ) {
        (self.ordinary, self.compositions)
    }
}

#[async_trait]
pub trait IMcpGatewaySnapshotRepository: Send + Sync {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        observed_at: DateTime<Utc>,
        after_gateway_scope_id: Option<GatewayScopeId>,
        limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError>;

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError>;

    async fn mcp_gateway_active_scopes(
        &self,
        node_id: NodeId,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<GatewayScope>, RepositoryError>;

    async fn mcp_gateway_snapshot_inputs(
        &self,
        node_id: NodeId,
    ) -> Result<McpGatewaySnapshotInputs, RepositoryError>;

    async fn stage_mcp_gateway_snapshot(
        &self,
        stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError>;

    async fn stage_managed_route_publication(
        &self,
        _stage: StageManagedRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "managed Route publication staging is not implemented".into(),
        ))
    }

    async fn stage_managed_gateway_route_cutover(
        &self,
        _stage: StageManagedGatewayRouteCutover,
    ) -> Result<GatewayRouteCutoverResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "managed Gateway Route cutover staging is not implemented".into(),
        ))
    }

    async fn stage_managed_gateway_rollout(
        &self,
        _stage: StageManagedGatewayRollout,
    ) -> Result<GatewayRolloutResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "managed Gateway rollout staging is not implemented".into(),
        ))
    }

    async fn stage_managed_gateway_rollout_rollback(
        &self,
        _stage: StageManagedGatewayRolloutRollback,
    ) -> Result<GatewayRolloutRollbackResult, RepositoryError> {
        Err(RepositoryError::Storage(
            "managed Gateway rollout rollback staging is not implemented".into(),
        ))
    }

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
    candidate.ordinary_route_ids().to_vec()
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
