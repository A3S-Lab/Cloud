use super::postgres::{
    insert_publication, PostgresEdgeRepository, PublicationRow, PublicationSelection, RouteRow,
    RouteSelection,
};
use super::postgres_schema::{
    DomainClaims, GatewayCertificates, GatewayPublications, GatewayRouteProjections,
    GatewayRouteScopes, GatewayScopeMembers, GatewayScopes, McpCredentials,
    McpGatewaySnapshotHeads, McpGatewaySnapshotPublications, McpRoutePolicies, Nodes, Routes,
    Workloads,
};
use super::postgres_tls::{
    insert_certificate, update_certificate, CertificateRow, CertificateSelection,
};
use super::{postgres_gateway_scopes, postgres_rollout_routes};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::{GatewayPublicationState, GatewayScopeState, RouteState};
use crate::modules::edge::infrastructure::{
    CompiledMcpGatewaySnapshot, GatewayManagedSnapshotComposition, GatewaySnapshotPublicationOwner,
    IMcpGatewaySnapshotRepository, McpGatewayReconciliationScope, McpGatewaySnapshotDispatchTarget,
    McpGatewaySnapshotInputs, McpGatewaySnapshotReconciliationState, McpGatewaySnapshotStageResult,
    McpGatewaySnapshotStatus, StageManagedGatewayCertificateConvergence,
    StageManagedGatewayRollout, StageManagedGatewayRolloutRollback,
    StageManagedGatewayRouteCutover, StageManagedRoutePublication, StageMcpGatewaySnapshot,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayCertificateId, GatewayScopeId, McpCredentialId,
    NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId,
    WorkloadRevisionId,
};
use a3s_orm::expression::{exists, not, Selection};
use a3s_orm::{
    delete_from, insert_into, select_from, update_table, Database, DecodeError, Expression,
    FromRow, FromValue, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
    Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[async_trait]
impl IMcpGatewaySnapshotRepository for PostgresEdgeRepository {
    async fn mcp_gateway_reconciliation_scopes(
        &self,
        observed_at: DateTime<Utc>,
        after_gateway_scope_id: Option<GatewayScopeId>,
        limit: usize,
    ) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
        reconciliation_scopes(&self.executor, observed_at, after_gateway_scope_id, limit).await
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        reconciliation_state(&self.executor, node_id).await
    }

    async fn mcp_gateway_installed_certificate(
        &self,
        node_id: NodeId,
    ) -> Result<Option<crate::modules::edge::domain::GatewayCertificate>, RepositoryError> {
        installed_certificate(&self.executor, node_id).await
    }

    async fn mcp_gateway_active_scopes(
        &self,
        node_id: NodeId,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<crate::modules::edge::domain::GatewayScope>, RepositoryError> {
        active_scopes(&self.executor, node_id, observed_at).await
    }

    async fn mcp_gateway_snapshot_inputs(
        &self,
        node_id: NodeId,
    ) -> Result<McpGatewaySnapshotInputs, RepositoryError> {
        let physical_scope = <Self as IEdgeRepository>::gateway_scope(self, node_id).await?;
        let routes = <Self as IEdgeRepository>::active_routes(self, node_id).await?;
        let mut active_routes = Vec::with_capacity(routes.len());
        for route in routes {
            let claim_id = route.domain_claim_id.ok_or_else(|| {
                RepositoryError::Storage(
                    "active ordinary Gateway Route omitted its DomainClaim".into(),
                )
            })?;
            let domain_claim =
                <Self as IEdgeRepository>::find_domain_claim(self, route.organization_id, claim_id)
                    .await?;
            active_routes.push(
                crate::modules::edge::infrastructure::GatewaySnapshotRouteInput {
                    route,
                    domain_claim,
                },
            );
        }
        let inputs = McpGatewaySnapshotInputs {
            physical_scope,
            active_routes,
        };
        inputs.validate(node_id).map_err(RepositoryError::Storage)?;
        Ok(inputs)
    }

    async fn stage_mcp_gateway_snapshot(
        &self,
        stage: StageMcpGatewaySnapshot,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        stage.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let (composition, publication, certificate) = stage.into_parts();
                    let physical_scope =
                        lock_managed_composition(transaction, &composition).await?;
                    if fetch_optional::<u64, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(GatewayPublications::revision())
                            .filter(
                                GatewayPublications::node_id().eq(publication.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::state().eq("pending"))
                            .for_update(),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway scope already has a pending complete snapshot".into(),
                        )
                        .into());
                    }

                    insert_publication(transaction, &publication).await?;
                    if let Some(certificate) = &certificate {
                        insert_certificate(transaction, certificate).await?;
                    }
                    persist_managed_composition(transaction, &composition, &publication).await?;
                    advance_physical_scope(transaction, &physical_scope, &publication).await?;
                    Ok(McpGatewaySnapshotStageResult {
                        publication,
                        certificate,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn stage_managed_route_publication(
        &self,
        stage: StageManagedRoutePublication,
    ) -> Result<
        crate::modules::edge::domain::repositories::EdgeRoutePublicationResult,
        RepositoryError,
    > {
        super::postgres::stage_managed_route_publication(&self.executor, stage).await
    }

    async fn stage_managed_gateway_route_cutover(
        &self,
        stage: StageManagedGatewayRouteCutover,
    ) -> Result<
        crate::modules::edge::domain::repositories::GatewayRouteCutoverResult,
        RepositoryError,
    > {
        super::postgres_cutovers::stage_managed(&self.executor, stage).await
    }

    async fn stage_managed_gateway_certificate_convergence(
        &self,
        stage: StageManagedGatewayCertificateConvergence,
    ) -> Result<
        crate::modules::edge::domain::repositories::GatewayCertificateConvergenceResult,
        RepositoryError,
    > {
        super::postgres_certificate_convergence::stage_managed(&self.executor, stage).await
    }

    async fn stage_managed_gateway_rollout(
        &self,
        stage: StageManagedGatewayRollout,
    ) -> Result<crate::modules::edge::domain::repositories::GatewayRolloutResult, RepositoryError>
    {
        super::postgres_rollouts::stage_managed(&self.executor, stage).await
    }

    async fn stage_managed_gateway_rollout_rollback(
        &self,
        stage: StageManagedGatewayRolloutRollback,
    ) -> Result<
        crate::modules::edge::domain::repositories::GatewayRolloutRollbackResult,
        RepositoryError,
    > {
        super::postgres_rollouts::stage_managed_rollback(&self.executor, stage).await
    }

    async fn pending_mcp_gateway_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
        pending(&self.executor, limit).await
    }

    async fn mark_mcp_gateway_snapshot_unavailable(
        &self,
        organization_id: OrganizationId,
        gateway_scope_id: GatewayScopeId,
        node_id: NodeId,
        gateway_revision: u64,
        gateway_command_id: NodeCommandId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
        mark_unavailable(
            &self.executor,
            organization_id,
            gateway_scope_id,
            node_id,
            gateway_revision,
            gateway_command_id,
            failure,
            observed_at,
        )
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpGatewaySnapshotMarker {
    pub(super) organization_id: OrganizationId,
    pub(super) project_id: ProjectId,
    pub(super) environment_id: EnvironmentId,
    pub(super) gateway_scope_id: GatewayScopeId,
    pub(super) node_id: NodeId,
    pub(super) gateway_revision: u64,
    pub(super) gateway_command_id: NodeCommandId,
    pub(super) snapshot_digest: String,
    pub(super) desired_state_digest: crate::modules::shared_kernel::domain::Sha256Digest,
    pub(super) desired_gateway_scope_ids: Vec<GatewayScopeId>,
    pub(super) mcp_route_count: u32,
    pub(super) publication_owner: GatewaySnapshotPublicationOwner,
    pub(super) staged_at: DateTime<Utc>,
}

impl McpGatewaySnapshotMarker {
    pub(super) fn validate_for(
        &self,
        publication: &crate::modules::edge::domain::GatewayPublication,
    ) -> Result<(), String> {
        publication.snapshot()?;
        crate::modules::shared_kernel::domain::Sha256Digest::parse(self.snapshot_digest.clone())?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.gateway_revision == 0
            || self
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
            || self.node_id != publication.node_id
            || self.gateway_revision != publication.revision
            || self.gateway_command_id != publication.command_id
            || self.snapshot_digest != publication.snapshot_digest
            || self.staged_at != publication.command_issued_at
        {
            return Err("stored MCP Gateway snapshot publication identity is inconsistent".into());
        }
        Ok(())
    }

    fn dispatch_target(
        &self,
        publication: crate::modules::edge::domain::GatewayPublication,
    ) -> Result<McpGatewaySnapshotDispatchTarget, RepositoryError> {
        self.validate_for(&publication)
            .map_err(RepositoryError::Storage)?;
        if self.publication_owner != GatewaySnapshotPublicationOwner::McpReconciler {
            return Err(RepositoryError::Storage(
                "ordinary Gateway publication cannot be dispatched by the MCP reconciler".into(),
            ));
        }
        let target = McpGatewaySnapshotDispatchTarget {
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            gateway_scope_id: self.gateway_scope_id,
            publication,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        Ok(target)
    }
}

struct McpGatewaySnapshotMarkerRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_scope_id: Uuid,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    snapshot_digest: String,
    desired_state_digest: String,
    desired_gateway_scope_ids: serde_json::Value,
    mcp_route_count: u32,
    publication_owner: String,
    staged_at: DateTime<Utc>,
}

struct McpGatewaySnapshotMarkerSelection;

impl Selection for McpGatewaySnapshotMarkerSelection {
    type Output = McpGatewaySnapshotMarkerRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            McpGatewaySnapshotPublications::organization_id().expression(),
            McpGatewaySnapshotPublications::project_id().expression(),
            McpGatewaySnapshotPublications::environment_id().expression(),
            McpGatewaySnapshotPublications::gateway_scope_id().expression(),
            McpGatewaySnapshotPublications::node_id().expression(),
            McpGatewaySnapshotPublications::gateway_revision().expression(),
            McpGatewaySnapshotPublications::gateway_command_id().expression(),
            McpGatewaySnapshotPublications::snapshot_digest().expression(),
            McpGatewaySnapshotPublications::desired_state_digest().expression(),
            McpGatewaySnapshotPublications::desired_gateway_scope_ids().expression(),
            McpGatewaySnapshotPublications::mcp_route_count().expression(),
            McpGatewaySnapshotPublications::publication_owner().expression(),
            McpGatewaySnapshotPublications::staged_at().expression(),
        ]
    }
}

impl FromRow for McpGatewaySnapshotMarkerRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Self::from_row_at(row, 0)
    }
}

impl McpGatewaySnapshotMarkerRow {
    fn from_row_at(row: &impl Row, offset: usize) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, offset)?,
            project_id: decode(row, offset + 1)?,
            environment_id: decode(row, offset + 2)?,
            gateway_scope_id: decode(row, offset + 3)?,
            node_id: decode(row, offset + 4)?,
            gateway_revision: decode(row, offset + 5)?,
            gateway_command_id: decode(row, offset + 6)?,
            snapshot_digest: decode(row, offset + 7)?,
            desired_state_digest: decode(row, offset + 8)?,
            desired_gateway_scope_ids: decode(row, offset + 9)?,
            mcp_route_count: decode(row, offset + 10)?,
            publication_owner: decode(row, offset + 11)?,
            staged_at: decode(row, offset + 12)?,
        })
    }

    fn marker(self) -> Result<McpGatewaySnapshotMarker, RepositoryError> {
        Ok(McpGatewaySnapshotMarker {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            node_id: NodeId::from_uuid(self.node_id),
            gateway_revision: self.gateway_revision,
            gateway_command_id: NodeCommandId::from_uuid(self.gateway_command_id),
            snapshot_digest: self.snapshot_digest,
            desired_state_digest: crate::modules::shared_kernel::domain::Sha256Digest::parse(
                self.desired_state_digest,
            )
            .map_err(RepositoryError::Storage)?,
            desired_gateway_scope_ids: serde_json::from_value::<Vec<Uuid>>(
                self.desired_gateway_scope_ids,
            )
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .into_iter()
            .map(GatewayScopeId::from_uuid)
            .collect(),
            mcp_route_count: self.mcp_route_count,
            publication_owner: GatewaySnapshotPublicationOwner::parse(&self.publication_owner)
                .map_err(RepositoryError::Storage)?,
            staged_at: self.staged_at,
        })
    }
}

struct McpGatewaySnapshotDispatchSelection;

impl Selection for McpGatewaySnapshotDispatchSelection {
    type Output = McpGatewaySnapshotDispatchRow;

    fn expressions(self) -> Vec<Expression> {
        let mut expressions = PublicationSelection.expressions();
        expressions.extend(McpGatewaySnapshotMarkerSelection.expressions());
        expressions
    }
}

struct McpGatewaySnapshotDispatchRow {
    publication: PublicationRow,
    marker: McpGatewaySnapshotMarkerRow,
}

impl FromRow for McpGatewaySnapshotDispatchRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            publication: PublicationRow::from_row(row)?,
            marker: McpGatewaySnapshotMarkerRow::from_row_at(row, 14)?,
        })
    }
}

impl McpGatewaySnapshotDispatchRow {
    fn status(self) -> Result<McpGatewaySnapshotStatus, RepositoryError> {
        let publication = self.publication.publication()?;
        let marker = self.marker.marker()?;
        marker
            .validate_for(&publication)
            .map_err(RepositoryError::Storage)?;
        let status = McpGatewaySnapshotStatus {
            organization_id: marker.organization_id,
            project_id: marker.project_id,
            environment_id: marker.environment_id,
            gateway_scope_id: marker.gateway_scope_id,
            desired_state_digest: marker.desired_state_digest,
            desired_gateway_scope_ids: marker.desired_gateway_scope_ids,
            mcp_route_count: marker.mcp_route_count,
            publication,
        };
        status.validate().map_err(RepositoryError::Storage)?;
        Ok(status)
    }
}

async fn reconciliation_scopes(
    executor: &PostgresExecutor,
    observed_at: DateTime<Utc>,
    after_gateway_scope_id: Option<GatewayScopeId>,
    limit: usize,
) -> Result<Vec<McpGatewayReconciliationScope>, RepositoryError> {
    validate_dispatch_limit(limit)?;
    if after_gateway_scope_id.is_some_and(|scope_id| scope_id.as_uuid().is_nil()) {
        return Err(RepositoryError::Conflict(
            "MCP Gateway reconciliation cursor is invalid".into(),
        ));
    }
    let observed_at = canonical_timestamp(observed_at);
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("MCP Gateway reconciliation scope limit is invalid".into())
    })?;
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(observed_at)),
    );
    let prior_mcp_publication = exists(
        select_from::<McpGatewaySnapshotHeads>()
            .inner_join::<McpGatewaySnapshotPublications>(
                McpGatewaySnapshotHeads::node_id()
                    .eq_column(McpGatewaySnapshotPublications::node_id())
                    .and(
                        McpGatewaySnapshotHeads::gateway_revision()
                            .eq_column(McpGatewaySnapshotPublications::gateway_revision()),
                    ),
            )
            .select(McpGatewaySnapshotHeads::gateway_revision())
            .filter(
                McpGatewaySnapshotPublications::gateway_scope_id()
                    .eq_column(GatewayRouteScopes::id()),
            ),
    );
    let query = select_from::<GatewayRouteScopes>()
        .select((
            GatewayRouteScopes::organization_id(),
            GatewayRouteScopes::id(),
        ))
        .filter(active_policy.or(prior_mcp_publication));
    let query = if let Some(after_gateway_scope_id) = after_gateway_scope_id {
        query.filter(GatewayRouteScopes::id().gt(after_gateway_scope_id.as_uuid()))
    } else {
        query
    };
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            query
                .order_by(GatewayRouteScopes::id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let database = Database::new(PostgresDialect, executor.clone());
    let mut scopes = Vec::with_capacity(identities.len());
    for (organization_id, scope_id) in identities {
        let scope = postgres_gateway_scopes::find(
            executor,
            OrganizationId::from_uuid(organization_id),
            GatewayScopeId::from_uuid(scope_id),
        )
        .await?;
        let has_active_policy = database
            .fetch_optional_as(
                select_from::<McpRoutePolicies>()
                    .select(McpRoutePolicies::id())
                    .filter(McpRoutePolicies::gateway_scope_id().eq(scope_id))
                    .filter(McpRoutePolicies::expires_at().gt(observed_at))
                    .limit(1),
            )
            .await
            .map_err(storage)?
            .is_some();
        let mut node_ids = BTreeSet::new();
        if has_active_policy {
            node_ids.extend(scope.member_node_ids.iter().copied());
        }
        node_ids.extend(
            database
                .fetch_all_as(
                    select_from::<McpGatewaySnapshotHeads>()
                        .inner_join::<McpGatewaySnapshotPublications>(
                            McpGatewaySnapshotHeads::node_id()
                                .eq_column(McpGatewaySnapshotPublications::node_id())
                                .and(
                                    McpGatewaySnapshotHeads::gateway_revision().eq_column(
                                        McpGatewaySnapshotPublications::gateway_revision(),
                                    ),
                                ),
                        )
                        .select(McpGatewaySnapshotHeads::node_id())
                        .filter(McpGatewaySnapshotPublications::gateway_scope_id().eq(scope_id))
                        .order_by(McpGatewaySnapshotHeads::node_id(), OrderDirection::Asc)
                        .limit(10_001),
                )
                .await
                .map_err(storage)?
                .rows
                .into_iter()
                .map(NodeId::from_uuid),
        );
        if node_ids.len() > 10_000 {
            return Err(RepositoryError::Storage(
                "MCP Gateway reconciliation scope exceeds the physical-node bound".into(),
            ));
        }
        let target = McpGatewayReconciliationScope {
            scope,
            node_ids: node_ids.into_iter().collect(),
        };
        target.validate().map_err(RepositoryError::Storage)?;
        scopes.push(target);
    }
    Ok(scopes)
}

async fn reconciliation_state(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
    if node_id.as_uuid().is_nil() {
        return Err(RepositoryError::Conflict(
            "MCP Gateway reconciliation identity is invalid".into(),
        ));
    }
    let database = Database::new(PostgresDialect, executor.clone());
    let pending_publication = database
        .fetch_optional_as(
            select_from::<GatewayPublications>()
                .select(GatewayPublications::revision())
                .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                .filter(GatewayPublications::state().eq("pending"))
                .limit(1),
        )
        .await
        .map_err(storage)?
        .is_some();
    let latest_mcp_snapshot = database
        .fetch_optional_as(
            select_from::<McpGatewaySnapshotHeads>()
                .inner_join::<McpGatewaySnapshotPublications>(
                    McpGatewaySnapshotHeads::node_id()
                        .eq_column(McpGatewaySnapshotPublications::node_id())
                        .and(
                            McpGatewaySnapshotHeads::gateway_revision()
                                .eq_column(McpGatewaySnapshotPublications::gateway_revision()),
                        ),
                )
                .inner_join::<GatewayPublications>(
                    McpGatewaySnapshotPublications::node_id()
                        .eq_column(GatewayPublications::node_id())
                        .and(
                            McpGatewaySnapshotPublications::gateway_revision()
                                .eq_column(GatewayPublications::revision()),
                        )
                        .and(
                            McpGatewaySnapshotPublications::gateway_command_id()
                                .eq_column(GatewayPublications::command_id()),
                        ),
                )
                .select(McpGatewaySnapshotDispatchSelection)
                .filter(McpGatewaySnapshotHeads::node_id().eq(node_id.as_uuid()))
                .limit(1),
        )
        .await
        .map_err(storage)?
        .map(McpGatewaySnapshotDispatchRow::status)
        .transpose()?;
    if latest_mcp_snapshot
        .as_ref()
        .is_some_and(|status| status.publication.node_id != node_id)
    {
        return Err(RepositoryError::Storage(
            "MCP Gateway reconciliation state crossed its requested identity".into(),
        ));
    }
    let state = McpGatewaySnapshotReconciliationState {
        pending_publication,
        latest_mcp_snapshot,
    };
    state.validate().map_err(RepositoryError::Storage)?;
    Ok(state)
}

async fn installed_certificate(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<Option<crate::modules::edge::domain::GatewayCertificate>, RepositoryError> {
    let state = reconciliation_state(executor, node_id).await?;
    let Some(status) = state.latest_mcp_snapshot else {
        return Ok(None);
    };
    if status.publication.state != GatewayPublicationState::Applied {
        return Ok(None);
    }
    let Some(request) = status.publication.certificate_request.as_ref() else {
        return Ok(None);
    };
    let certificate_id = GatewayCertificateId::from_uuid(request.certificate_id);
    let certificate =
        super::postgres_tls::find_gateway_certificate(executor, node_id, certificate_id).await?;
    if certificate.node_id != node_id
        || certificate.organization_id != status.organization_id
        || certificate.request != *request
    {
        return Err(RepositoryError::Storage(
            "installed MCP Gateway certificate projection is inconsistent".into(),
        ));
    }
    Ok(Some(certificate))
}

async fn active_scopes(
    executor: &PostgresExecutor,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> Result<Vec<crate::modules::edge::domain::GatewayScope>, RepositoryError> {
    if node_id.as_uuid().is_nil() {
        return Err(RepositoryError::Conflict(
            "MCP Gateway active-scope node identity is invalid".into(),
        ));
    }
    let observed_at = canonical_timestamp(observed_at);
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(observed_at)),
    );
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayRouteScopes>()
                .inner_join::<GatewayScopeMembers>(
                    GatewayRouteScopes::id().eq_column(GatewayScopeMembers::gateway_scope_id()),
                )
                .select((
                    GatewayRouteScopes::organization_id(),
                    GatewayRouteScopes::id(),
                ))
                .filter(GatewayScopeMembers::node_id().eq(node_id.as_uuid()))
                .filter(active_policy)
                .order_by(GatewayRouteScopes::id(), OrderDirection::Asc)
                .limit(1_001),
        )
        .await
        .map_err(storage)?
        .rows;
    if identities.len() > 1_000 {
        return Err(RepositoryError::Storage(
            "physical Gateway exceeds the active MCP logical-scope bound".into(),
        ));
    }
    let mut scopes = Vec::with_capacity(identities.len());
    for (organization_id, scope_id) in identities {
        let scope = postgres_gateway_scopes::find(
            executor,
            OrganizationId::from_uuid(organization_id),
            GatewayScopeId::from_uuid(scope_id),
        )
        .await?;
        if !scope.contains_member(node_id)
            || scopes
                .last()
                .is_some_and(|previous: &crate::modules::edge::domain::GatewayScope| {
                    previous.id >= scope.id
                })
        {
            return Err(RepositoryError::Storage(
                "physical Gateway active MCP scopes are not canonical".into(),
            ));
        }
        scopes.push(scope);
    }
    Ok(scopes)
}

pub(super) async fn lock_managed_composition(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
    lock_managed_composition_node(transaction, composition).await?;
    let candidate = composition.candidate();
    lock_logical_scopes(transaction, candidate).await?;
    let physical_scope = lock_physical_scope(transaction, candidate).await?;
    lock_ordinary_routes(transaction, candidate).await?;
    lock_mcp_policies(transaction, candidate).await?;
    lock_domain_claims(transaction, candidate).await?;
    lock_workloads(transaction, candidate).await?;
    lock_credentials(transaction, candidate).await?;
    Ok(physical_scope)
}

pub(super) async fn lock_managed_composition_node(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
) -> Result<(), PostgresPersistenceError> {
    let candidate = composition.candidate();
    let organization_id = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<Nodes>()
            .select(Nodes::organization_id())
            .filter(Nodes::id().eq(candidate.physical_scope().node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if organization_id != candidate.mcp().anchor().organization_id.as_uuid() {
        return Err(RepositoryError::NotFound.into());
    }
    Ok(())
}

pub(super) async fn persist_managed_composition(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    composition
        .validate_for(publication)
        .map_err(RepositoryError::Conflict)?;
    insert_marker(transaction, composition, publication).await?;
    advance_snapshot_head(transaction, composition.candidate(), publication).await?;
    store_outbox(transaction, composition.event()).await?;
    Ok(())
}

async fn insert_marker(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let candidate = composition.candidate();
    let anchor = candidate.mcp().anchor();
    let mcp_route_count = u32::try_from(candidate.mcp().route_versions().len()).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "MCP Gateway snapshot route count exceeds durable bounds".into(),
        )
    })?;
    require_one_row(
        "MCP Gateway snapshot publication marker",
        execute(
            transaction,
            insert_into::<McpGatewaySnapshotPublications>()
                .value(
                    McpGatewaySnapshotPublications::organization_id(),
                    anchor.organization_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::project_id(),
                    anchor.project_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::environment_id(),
                    anchor.environment_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::gateway_scope_id(),
                    anchor.gateway_scope_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::node_id(),
                    publication.node_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::gateway_revision(),
                    publication.revision,
                )
                .value(
                    McpGatewaySnapshotPublications::gateway_command_id(),
                    publication.command_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::snapshot_digest(),
                    publication.snapshot_digest.clone(),
                )
                .value(
                    McpGatewaySnapshotPublications::desired_state_digest(),
                    candidate.desired_state_digest().as_str(),
                )
                .value(
                    McpGatewaySnapshotPublications::desired_gateway_scope_ids(),
                    serde_json::to_value(
                        candidate
                            .mcp()
                            .scope_ids()
                            .iter()
                            .map(|scope_id| scope_id.as_uuid())
                            .collect::<Vec<_>>(),
                    )?,
                )
                .value(
                    McpGatewaySnapshotPublications::mcp_route_count(),
                    mcp_route_count,
                )
                .value(
                    McpGatewaySnapshotPublications::publication_owner(),
                    composition.owner().as_str(),
                )
                .value(
                    McpGatewaySnapshotPublications::staged_at(),
                    publication.command_issued_at,
                ),
        )
        .await?,
    )?;
    Ok(())
}

async fn advance_snapshot_head(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let current = fetch_optional::<u64, _>(
        transaction,
        select_from::<McpGatewaySnapshotHeads>()
            .select(McpGatewaySnapshotHeads::gateway_revision())
            .filter(McpGatewaySnapshotHeads::node_id().eq(publication.node_id.as_uuid()))
            .for_update(),
    )
    .await?;
    match current {
        Some(current) if current >= publication.revision => {
            return Err(RepositoryError::Conflict(
                "MCP Gateway snapshot head cannot regress".into(),
            )
            .into())
        }
        Some(current) => {
            require_one_row(
                "MCP Gateway snapshot head",
                execute(
                    transaction,
                    update_table::<McpGatewaySnapshotHeads>()
                        .set(
                            McpGatewaySnapshotHeads::organization_id(),
                            candidate.mcp().organization_id().as_uuid(),
                        )
                        .set(
                            McpGatewaySnapshotHeads::gateway_revision(),
                            publication.revision,
                        )
                        .set(
                            McpGatewaySnapshotHeads::advanced_at(),
                            publication.command_issued_at,
                        )
                        .filter(
                            McpGatewaySnapshotHeads::node_id().eq(publication.node_id.as_uuid()),
                        )
                        .filter(McpGatewaySnapshotHeads::gateway_revision().eq(current)),
                )
                .await?,
            )?;
        }
        None => {
            require_one_row(
                "MCP Gateway snapshot head",
                execute(
                    transaction,
                    insert_into::<McpGatewaySnapshotHeads>()
                        .value(
                            McpGatewaySnapshotHeads::organization_id(),
                            candidate.mcp().organization_id().as_uuid(),
                        )
                        .value(
                            McpGatewaySnapshotHeads::node_id(),
                            publication.node_id.as_uuid(),
                        )
                        .value(
                            McpGatewaySnapshotHeads::gateway_revision(),
                            publication.revision,
                        )
                        .value(
                            McpGatewaySnapshotHeads::advanced_at(),
                            publication.command_issued_at,
                        ),
                )
                .await?,
            )?;
        }
    }
    Ok(())
}

async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<McpGatewaySnapshotDispatchTarget>, RepositoryError> {
    validate_dispatch_limit(limit)?;
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("MCP Gateway snapshot dispatch limit is invalid".into())
    })?;
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<McpGatewaySnapshotPublications>()
                .inner_join::<GatewayPublications>(
                    McpGatewaySnapshotPublications::node_id()
                        .eq_column(GatewayPublications::node_id())
                        .and(
                            McpGatewaySnapshotPublications::gateway_revision()
                                .eq_column(GatewayPublications::revision()),
                        )
                        .and(
                            McpGatewaySnapshotPublications::gateway_command_id()
                                .eq_column(GatewayPublications::command_id()),
                        ),
                )
                .select(McpGatewaySnapshotDispatchSelection)
                .filter(GatewayPublications::state().eq("pending"))
                .filter(McpGatewaySnapshotPublications::publication_owner().eq("mcp-reconciler"))
                .order_by(
                    McpGatewaySnapshotPublications::staged_at(),
                    OrderDirection::Asc,
                )
                .order_by(
                    McpGatewaySnapshotPublications::node_id(),
                    OrderDirection::Asc,
                )
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    rows.into_iter()
        .map(|row| {
            row.marker
                .marker()?
                .dispatch_target(row.publication.publication()?)
        })
        .collect()
}

pub(super) async fn lock_marker_by_gateway_identity(
    transaction: &PostgresTransaction,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
) -> Result<Option<McpGatewaySnapshotMarker>, PostgresPersistenceError> {
    let row = fetch_optional::<McpGatewaySnapshotMarkerRow, _>(
        transaction,
        select_from::<McpGatewaySnapshotPublications>()
            .select(McpGatewaySnapshotMarkerSelection)
            .filter(McpGatewaySnapshotPublications::node_id().eq(node_id))
            .filter(McpGatewaySnapshotPublications::gateway_revision().eq(gateway_revision))
            .filter(McpGatewaySnapshotPublications::gateway_command_id().eq(gateway_command_id))
            .for_update(),
    )
    .await?;
    row.map(McpGatewaySnapshotMarkerRow::marker)
        .transpose()
        .map_err(PostgresPersistenceError::from)
}

pub(super) async fn validate_stored_managed_composition(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    composition
        .validate_for(publication)
        .map_err(PostgresPersistenceError::Invariant)?;
    let marker = lock_marker_by_gateway_identity(
        transaction,
        publication.node_id.as_uuid(),
        publication.revision,
        publication.command_id.as_uuid(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "managed Gateway publication lost its immutable composition marker".into(),
        )
    })?;
    marker
        .validate_for(publication)
        .map_err(PostgresPersistenceError::Invariant)?;
    let candidate = composition.candidate();
    let expected_route_count =
        u32::try_from(candidate.mcp().route_versions().len()).map_err(|_| {
            PostgresPersistenceError::Invariant(
                "managed Gateway composition route count exceeds durable bounds".into(),
            )
        })?;
    if marker.organization_id != candidate.mcp().anchor().organization_id
        || marker.project_id != candidate.mcp().anchor().project_id
        || marker.environment_id != candidate.mcp().anchor().environment_id
        || marker.gateway_scope_id != candidate.mcp().anchor().gateway_scope_id
        || marker.desired_state_digest != *candidate.desired_state_digest()
        || marker.desired_gateway_scope_ids != candidate.mcp().scope_ids()
        || marker.mcp_route_count != expected_route_count
        || marker.publication_owner != composition.owner()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored managed Gateway composition changed".into(),
        ));
    }
    Ok(())
}

pub(super) async fn release_snapshot_head(
    transaction: &PostgresTransaction,
    marker: &McpGatewaySnapshotMarker,
) -> Result<(), PostgresPersistenceError> {
    if marker.mcp_route_count != 0 {
        return Err(PostgresPersistenceError::Invariant(
            "non-empty MCP Gateway snapshot cannot release node ownership".into(),
        ));
    }
    require_one_row(
        "empty MCP Gateway snapshot head",
        execute(
            transaction,
            delete_from::<McpGatewaySnapshotHeads>()
                .filter(McpGatewaySnapshotHeads::node_id().eq(marker.node_id.as_uuid()))
                .filter(McpGatewaySnapshotHeads::gateway_revision().eq(marker.gateway_revision)),
        )
        .await?,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn mark_unavailable(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    gateway_scope_id: GatewayScopeId,
    node_id: NodeId,
    gateway_revision: u64,
    gateway_command_id: NodeCommandId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<McpGatewaySnapshotStageResult, RepositoryError> {
    let failure = failure.to_owned();
    let observed_at = canonical_timestamp(observed_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut publication = fetch_optional::<PublicationRow, _>(
                    transaction,
                    select_from::<GatewayPublications>()
                        .select(PublicationSelection)
                        .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                        .filter(GatewayPublications::revision().eq(gateway_revision))
                        .for_update(),
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "MCP Gateway snapshot publication disappeared".into(),
                    )
                })?
                .publication()?;
                let marker = lock_marker_by_gateway_identity(
                    transaction,
                    node_id.as_uuid(),
                    gateway_revision,
                    gateway_command_id.as_uuid(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if marker.organization_id != organization_id
                    || marker.gateway_scope_id != gateway_scope_id
                {
                    return Err(RepositoryError::NotFound.into());
                }
                marker
                    .validate_for(&publication)
                    .map_err(PostgresPersistenceError::Invariant)?;
                if marker.publication_owner != GatewaySnapshotPublicationOwner::McpReconciler {
                    return Err(RepositoryError::Conflict(
                        "ordinary Gateway publication is owned by its originating reconciler"
                            .into(),
                    )
                    .into());
                }
                let mut certificates = fetch_all::<CertificateRow, _>(
                    transaction,
                    select_from::<GatewayCertificates>()
                        .select(CertificateSelection)
                        .filter(GatewayCertificates::node_id().eq(node_id.as_uuid()))
                        .filter(GatewayCertificates::gateway_revision().eq(gateway_revision))
                        .filter(
                            GatewayCertificates::gateway_command_id()
                                .eq(gateway_command_id.as_uuid()),
                        )
                        .for_update(),
                )
                .await?
                .into_iter()
                .map(CertificateRow::certificate)
                .collect::<Result<Vec<_>, _>>()?;
                let expected_certificate_id = publication
                    .certificate_request
                    .as_ref()
                    .map(|request| GatewayCertificateId::from_uuid(request.certificate_id));
                if certificates.len() != usize::from(expected_certificate_id.is_some())
                    || certificates.first().map(|certificate| certificate.id)
                        != expected_certificate_id
                    || certificates.first().is_some_and(|certificate| {
                        certificate.organization_id != organization_id
                            || certificate.node_id != node_id
                            || certificate.gateway_revision != gateway_revision
                            || certificate.gateway_command_id != gateway_command_id
                            || certificate.snapshot_digest != publication.snapshot_digest
                    })
                {
                    return Err(PostgresPersistenceError::Invariant(
                        "MCP Gateway snapshot certificate projection is inconsistent".into(),
                    ));
                }
                let mut certificate = certificates.pop();
                let certificate_version = certificate
                    .as_ref()
                    .map(|certificate| certificate.aggregate_version);
                let publication_changed = publication
                    .mark_unavailable(&failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;
                let certificate_changed = match &mut certificate {
                    Some(certificate) => certificate
                        .mark_delivery_unavailable(&failure, observed_at)
                        .map_err(RepositoryError::Conflict)?,
                    None => false,
                };
                if publication_changed {
                    require_one_row(
                        "MCP Gateway snapshot unavailable publication",
                        execute(
                            transaction,
                            update_table::<GatewayPublications>()
                                .set(GatewayPublications::state(), publication.state.as_str())
                                .set(GatewayPublications::failure(), publication.failure.clone())
                                .set(
                                    GatewayPublications::acknowledged_at(),
                                    publication.acknowledged_at,
                                )
                                .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                                .filter(GatewayPublications::revision().eq(gateway_revision))
                                .filter(GatewayPublications::state().eq("pending")),
                        )
                        .await?,
                    )?;
                }
                if certificate_changed {
                    update_certificate(
                        transaction,
                        certificate.as_ref().ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "changed MCP Gateway snapshot certificate disappeared".into(),
                            )
                        })?,
                        certificate_version.ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "changed MCP Gateway snapshot certificate omitted its version"
                                    .into(),
                            )
                        })?,
                    )
                    .await?;
                }
                Ok(McpGatewaySnapshotStageResult {
                    publication,
                    certificate,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn lock_logical_scopes(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let anchor = candidate.mcp().anchor();
    for scope in candidate.mcp().scopes() {
        lock_one_logical_scope(transaction, scope).await?;
    }
    if candidate
        .mcp()
        .scopes()
        .first()
        .is_none_or(|scope| scope.id != anchor.gateway_scope_id)
    {
        let stored = fetch_optional::<(Uuid, Uuid, Uuid), _>(
            transaction,
            select_from::<GatewayRouteScopes>()
                .select((
                    GatewayRouteScopes::organization_id(),
                    GatewayRouteScopes::project_id(),
                    GatewayRouteScopes::environment_id(),
                ))
                .filter(GatewayRouteScopes::id().eq(anchor.gateway_scope_id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or(RepositoryError::NotFound)?;
        if stored
            != (
                anchor.organization_id.as_uuid(),
                anchor.project_id.as_uuid(),
                anchor.environment_id.as_uuid(),
            )
        {
            return Err(RepositoryError::Conflict(
                "historical MCP Gateway snapshot anchor changed while planning".into(),
            )
            .into());
        }
    }
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(candidate.mcp().observed_at())),
    );
    let active_scope_ids = fetch_all::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .inner_join::<GatewayScopeMembers>(
                GatewayRouteScopes::id().eq_column(GatewayScopeMembers::gateway_scope_id()),
            )
            .select(GatewayRouteScopes::id())
            .filter(GatewayScopeMembers::node_id().eq(candidate.mcp().gateway_node_id().as_uuid()))
            .filter(active_policy)
            .order_by(GatewayRouteScopes::id(), OrderDirection::Asc)
            .limit(1_001)
            .for_update(),
    )
    .await?;
    if active_scope_ids.len() != candidate.mcp().scope_ids().len()
        || active_scope_ids
            .iter()
            .zip(candidate.mcp().scope_ids())
            .any(|(stored, expected)| *stored != expected.as_uuid())
    {
        return Err(RepositoryError::Conflict(
            "physical Gateway active MCP scope set changed while planning".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_one_logical_scope(
    transaction: &PostgresTransaction,
    scope: &crate::modules::edge::domain::GatewayScope,
) -> Result<(), PostgresPersistenceError> {
    let stored = fetch_optional::<(Uuid, Uuid, Uuid, Uuid, u64, u64), _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .select((
                GatewayRouteScopes::organization_id(),
                GatewayRouteScopes::project_id(),
                GatewayRouteScopes::environment_id(),
                GatewayRouteScopes::node_id(),
                GatewayRouteScopes::membership_generation(),
                GatewayRouteScopes::aggregate_version(),
            ))
            .filter(GatewayRouteScopes::id().eq(scope.id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if stored
        != (
            scope.organization_id.as_uuid(),
            scope.project_id.as_uuid(),
            scope.environment_id.as_uuid(),
            scope.node_id.as_uuid(),
            scope.membership_generation,
            scope.aggregate_version,
        )
    {
        return Err(RepositoryError::Conflict(
            "logical Gateway scope changed while planning the MCP snapshot".into(),
        )
        .into());
    }
    let members = fetch_all::<(Uuid, u32, u64), _>(
        transaction,
        select_from::<GatewayScopeMembers>()
            .select((
                GatewayScopeMembers::node_id(),
                GatewayScopeMembers::ordinal(),
                GatewayScopeMembers::membership_generation(),
            ))
            .filter(GatewayScopeMembers::gateway_scope_id().eq(scope.id.as_uuid()))
            .filter(GatewayScopeMembers::organization_id().eq(scope.organization_id.as_uuid()))
            .filter(GatewayScopeMembers::project_id().eq(scope.project_id.as_uuid()))
            .filter(GatewayScopeMembers::environment_id().eq(scope.environment_id.as_uuid()))
            .order_by(GatewayScopeMembers::ordinal(), OrderDirection::Asc)
            .for_update(),
    )
    .await?;
    if members.len() != scope.member_node_ids.len()
        || members.iter().zip(&scope.member_node_ids).enumerate().any(
            |(index, ((node_id, ordinal, generation), expected_node_id))| {
                *node_id != expected_node_id.as_uuid()
                    || usize::try_from(*ordinal).ok() != Some(index)
                    || *generation != scope.membership_generation
            },
        )
    {
        return Err(RepositoryError::Conflict(
            "logical Gateway membership changed while planning the MCP snapshot".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_physical_scope(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
    let expected = candidate.physical_scope();
    let row = fetch_optional::<(u64, Option<u64>, u64), _>(
        transaction,
        select_from::<GatewayScopes>()
            .select((
                GatewayScopes::last_issued_revision(),
                GatewayScopes::installed_revision(),
                GatewayScopes::aggregate_version(),
            ))
            .filter(GatewayScopes::node_id().eq(expected.node_id.as_uuid()))
            .for_update(),
    )
    .await?;
    let current = row
        .map(
            |(last_issued_revision, installed_revision, aggregate_version)| GatewayScopeState {
                node_id: expected.node_id,
                last_issued_revision,
                installed_revision,
                aggregate_version,
            },
        )
        .unwrap_or_else(|| GatewayScopeState::empty(expected.node_id));
    validate_physical_scope(&current)?;
    if &current != expected {
        return Err(RepositoryError::Conflict(
            "physical Gateway scope changed while compiling the complete snapshot".into(),
        )
        .into());
    }
    Ok(current)
}

async fn lock_ordinary_routes(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let node_id = candidate.physical_scope().node_id;
    let projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
    );
    let mut active = fetch_all::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .filter(not(projected_route))
            .order_by(Routes::id(), OrderDirection::Asc)
            .for_update(),
    )
    .await?
    .into_iter()
    .map(RouteRow::route)
    .collect::<Result<Vec<_>, _>>()?;
    active.extend(postgres_rollout_routes::lock_active(transaction, node_id).await?);
    active.sort_by_key(|route| route.id);
    let expected = candidate.active_route_versions();
    if active.len() != expected.len()
        || active.iter().zip(expected).any(|(route, expected)| {
            route.state != RouteState::Active
                || route.gateway_node_id != node_id
                || route.id != expected.route_id
                || route.aggregate_version != expected.aggregate_version
        })
    {
        return Err(RepositoryError::Conflict(
            "ordinary Gateway Route set changed while compiling the complete snapshot".into(),
        )
        .into());
    }
    Ok(())
}

async fn lock_mcp_policies(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    for scope in candidate.mcp().scopes() {
        let rows = fetch_all::<(Uuid, u64, String, Uuid, Uuid, DateTime<Utc>), _>(
            transaction,
            select_from::<McpRoutePolicies>()
                .select((
                    McpRoutePolicies::id(),
                    McpRoutePolicies::policy_revision(),
                    McpRoutePolicies::policy_digest(),
                    McpRoutePolicies::workload_id(),
                    McpRoutePolicies::domain_claim_id(),
                    McpRoutePolicies::updated_at(),
                ))
                .filter(McpRoutePolicies::organization_id().eq(scope.organization_id.as_uuid()))
                .filter(McpRoutePolicies::project_id().eq(scope.project_id.as_uuid()))
                .filter(McpRoutePolicies::environment_id().eq(scope.environment_id.as_uuid()))
                .filter(McpRoutePolicies::gateway_scope_id().eq(scope.id.as_uuid()))
                .filter(McpRoutePolicies::expires_at().gt(candidate.mcp().observed_at()))
                .order_by(McpRoutePolicies::id(), OrderDirection::Asc)
                .limit(1_001)
                .for_update(),
        )
        .await?;
        let expected = candidate
            .mcp()
            .observed_route_versions()
            .iter()
            .filter(|version| version.gateway_scope_id() == scope.id)
            .collect::<Vec<_>>();
        if rows.len() != expected.len()
            || rows.iter().zip(expected).any(
                |(
                    (route_id, revision, digest, workload_id, domain_claim_id, updated_at),
                    expected,
                )| {
                    *route_id != expected.route_id().as_uuid()
                        || *revision != expected.policy_revision()
                        || digest != expected.policy_digest().as_str()
                        || *workload_id != expected.workload_id().as_uuid()
                        || *domain_claim_id != expected.domain_claim_id().as_uuid()
                        || *updated_at > candidate.mcp().observed_at()
                },
            )
        {
            return Err(RepositoryError::Conflict(
                "active MCP route-policy set changed while compiling the complete snapshot".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn lock_domain_claims(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    for expected in candidate.domain_claim_versions() {
        let row = fetch_optional::<
            (
                Uuid,
                Uuid,
                Uuid,
                String,
                Option<String>,
                u64,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            ),
            _,
        >(
            transaction,
            select_from::<DomainClaims>()
                .select((
                    DomainClaims::organization_id(),
                    DomainClaims::project_id(),
                    DomainClaims::environment_id(),
                    DomainClaims::state(),
                    DomainClaims::failure(),
                    DomainClaims::aggregate_version(),
                    DomainClaims::updated_at(),
                    DomainClaims::revoked_at(),
                ))
                .filter(DomainClaims::id().eq(expected.domain_claim_id().as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "Gateway snapshot DomainClaim disappeared before staging".into(),
            )
        })?;
        let (
            organization_id,
            project_id,
            environment_id,
            state,
            failure,
            aggregate_version,
            updated_at,
            revoked_at,
        ) = row;
        if organization_id != expected.organization_id().as_uuid()
            || project_id != expected.project_id().as_uuid()
            || environment_id != expected.environment_id().as_uuid()
            || state != "verified"
            || failure.is_some()
            || aggregate_version != expected.aggregate_version()
            || updated_at > candidate.mcp().observed_at()
            || revoked_at.is_some()
        {
            return Err(RepositoryError::Conflict(
                "Gateway snapshot DomainClaim authority changed before staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn lock_workloads(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mut expected = BTreeMap::<
        WorkloadId,
        (
            OrganizationId,
            ProjectId,
            EnvironmentId,
            u64,
            WorkloadRevisionId,
        ),
    >::new();
    for version in candidate.mcp().observed_route_versions() {
        let scope = candidate
            .mcp()
            .scope(version.gateway_scope_id())
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "MCP snapshot Workload references an inactive logical scope".into(),
                )
            })?;
        let candidate_version = (
            scope.organization_id,
            scope.project_id,
            scope.environment_id,
            version.workload_aggregate_version(),
            version.active_revision_id(),
        );
        match expected.get(&version.workload_id()) {
            Some(value) if *value != candidate_version => {
                return Err(RepositoryError::Conflict(
                    "MCP snapshot observed conflicting versions of one Workload".into(),
                )
                .into())
            }
            Some(_) => {}
            None => {
                expected.insert(version.workload_id(), candidate_version);
            }
        }
    }
    for (
        workload_id,
        (organization_id, project_id, environment_id, aggregate_version, active_revision_id),
    ) in expected
    {
        let row = fetch_optional::<(Uuid, Uuid, Uuid, String, Option<Uuid>, u64), _>(
            transaction,
            select_from::<Workloads>()
                .select((
                    Workloads::organization_id(),
                    Workloads::project_id(),
                    Workloads::environment_id(),
                    Workloads::desired_state(),
                    Workloads::active_revision_id(),
                    Workloads::aggregate_version(),
                ))
                .filter(Workloads::id().eq(workload_id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "MCP snapshot Workload disappeared before Gateway staging".into(),
            )
        })?;
        if row
            != (
                organization_id.as_uuid(),
                project_id.as_uuid(),
                environment_id.as_uuid(),
                "running".to_owned(),
                Some(active_revision_id.as_uuid()),
                aggregate_version,
            )
        {
            return Err(RepositoryError::Conflict(
                "MCP snapshot Workload authority changed before Gateway staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

#[derive(Debug)]
struct McpCredentialLockExpectation {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    generation: u64,
    aggregate_version: u64,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    requires_active: bool,
    invalid_referenced_generations: BTreeSet<u64>,
}

impl McpCredentialLockExpectation {
    fn same_authority(&self, other: &Self) -> bool {
        self.organization_id == other.organization_id
            && self.project_id == other.project_id
            && self.environment_id == other.environment_id
            && self.generation == other.generation
            && self.aggregate_version == other.aggregate_version
            && self.expires_at == other.expires_at
            && self.revoked_at == other.revoked_at
    }
}

fn merge_mcp_credential_lock_expectation(
    expectations: &mut BTreeMap<McpCredentialId, McpCredentialLockExpectation>,
    credential_id: McpCredentialId,
    mut candidate: McpCredentialLockExpectation,
) -> Result<(), PostgresPersistenceError> {
    match expectations.get_mut(&credential_id) {
        Some(existing) if !existing.same_authority(&candidate) => {
            Err(PostgresPersistenceError::Invariant(
                "MCP snapshot observed conflicting authority for one credential".into(),
            ))
        }
        Some(existing) => {
            existing.requires_active |= candidate.requires_active;
            existing
                .invalid_referenced_generations
                .append(&mut candidate.invalid_referenced_generations);
            Ok(())
        }
        None => {
            expectations.insert(credential_id, candidate);
            Ok(())
        }
    }
}

async fn lock_credentials(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mut tenants = BTreeMap::<EnvironmentId, (OrganizationId, ProjectId)>::new();
    for scope in candidate.mcp().scopes() {
        match tenants.get(&scope.environment_id) {
            Some(existing) if *existing != (scope.organization_id, scope.project_id) => {
                return Err(PostgresPersistenceError::Invariant(
                    "MCP snapshot environment has conflicting tenant authority".into(),
                ))
            }
            Some(_) => {}
            None => {
                tenants.insert(
                    scope.environment_id,
                    (scope.organization_id, scope.project_id),
                );
            }
        }
    }

    let mut expectations = BTreeMap::<McpCredentialId, McpCredentialLockExpectation>::new();
    if let Some(projection) = candidate.mcp().projection() {
        for expected in projection.credential_versions() {
            let projected = projection
                .projection()
                .credentials
                .iter()
                .find(|credential| credential.credential_id == expected.credential_id().as_uuid())
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "MCP snapshot credential version lost its projection".into(),
                    )
                })?;
            let environment_id = EnvironmentId::from_uuid(projected.environment_id);
            let (organization_id, project_id) =
                tenants.get(&environment_id).copied().ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "MCP snapshot credential has no active-scope tenant".into(),
                    )
                })?;
            merge_mcp_credential_lock_expectation(
                &mut expectations,
                expected.credential_id(),
                McpCredentialLockExpectation {
                    organization_id,
                    project_id,
                    environment_id,
                    generation: expected.generation(),
                    aggregate_version: expected.aggregate_version(),
                    expires_at: projected.expires_at,
                    revoked_at: None,
                    requires_active: true,
                    invalid_referenced_generations: BTreeSet::new(),
                },
            )?;
        }
    }
    for suppression in candidate.mcp().credential_suppressions() {
        let scope = candidate
            .mcp()
            .scope(suppression.gateway_scope_id())
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "MCP credential suppression references an inactive logical scope".into(),
                )
            })?;
        if !suppression.is_invalid_at(candidate.mcp().observed_at())
            || candidate
                .mcp()
                .route_versions()
                .iter()
                .any(|version| version.route_id() == suppression.route_id())
            || !candidate
                .mcp()
                .observed_route_versions()
                .iter()
                .any(|version| {
                    version.route_id() == suppression.route_id()
                        && version.gateway_scope_id() == suppression.gateway_scope_id()
                })
        {
            return Err(PostgresPersistenceError::Invariant(
                "MCP credential suppression is not exact route-removal authority".into(),
            ));
        }
        let invalid_referenced_generations = BTreeSet::from([suppression.referenced_generation()]);
        merge_mcp_credential_lock_expectation(
            &mut expectations,
            suppression.credential_id(),
            McpCredentialLockExpectation {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.environment_id,
                generation: suppression.generation(),
                aggregate_version: suppression.aggregate_version(),
                expires_at: suppression.expires_at(),
                revoked_at: suppression.revoked_at(),
                requires_active: false,
                invalid_referenced_generations,
            },
        )?;
    }

    for (credential_id, expected) in expectations {
        let row = fetch_optional::<
            (
                Uuid,
                Uuid,
                Uuid,
                u64,
                u64,
                DateTime<Utc>,
                Option<DateTime<Utc>>,
            ),
            _,
        >(
            transaction,
            select_from::<McpCredentials>()
                .select((
                    McpCredentials::organization_id(),
                    McpCredentials::project_id(),
                    McpCredentials::environment_id(),
                    McpCredentials::generation(),
                    McpCredentials::aggregate_version(),
                    McpCredentials::expires_at(),
                    McpCredentials::revoked_at(),
                ))
                .filter(McpCredentials::id().eq(credential_id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| {
            RepositoryError::Conflict(
                "MCP snapshot credential disappeared before Gateway staging".into(),
            )
        })?;
        if row.0 != expected.organization_id.as_uuid()
            || row.1 != expected.project_id.as_uuid()
            || row.2 != expected.environment_id.as_uuid()
            || row.3 != expected.generation
            || row.4 != expected.aggregate_version
            || row.5 != expected.expires_at
            || row.6 != expected.revoked_at
            || expected.requires_active
                && (row.5 <= candidate.mcp().observed_at() || row.6.is_some())
            || expected
                .invalid_referenced_generations
                .iter()
                .any(|generation| {
                    row.3 == *generation && row.5 > candidate.mcp().observed_at() && row.6.is_none()
                })
        {
            return Err(RepositoryError::Conflict(
                "MCP snapshot credential authority changed before Gateway staging".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn advance_physical_scope(
    transaction: &PostgresTransaction,
    current: &GatewayScopeState,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    if publication.state != GatewayPublicationState::Pending
        || publication.node_id != current.node_id
        || publication.revision != current.next_revision().map_err(RepositoryError::Conflict)?
        || publication.expected_revision != current.installed_revision
    {
        return Err(RepositoryError::Conflict(
            "MCP Gateway publication does not advance its exact physical scope".into(),
        )
        .into());
    }
    let next_version = current.aggregate_version.checked_add(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant("Gateway scope aggregate version overflowed".into())
    })?;
    if current.aggregate_version == 0 {
        require_one_row(
            "MCP Gateway scope",
            execute(
                transaction,
                insert_into::<GatewayScopes>()
                    .value(GatewayScopes::node_id(), publication.node_id.as_uuid())
                    .value(GatewayScopes::last_issued_revision(), publication.revision)
                    .value(
                        GatewayScopes::installed_revision(),
                        current.installed_revision,
                    )
                    .value(GatewayScopes::aggregate_version(), next_version)
                    .value(GatewayScopes::updated_at(), publication.command_issued_at),
            )
            .await?,
        )?;
    } else {
        require_one_row(
            "MCP Gateway scope",
            execute(
                transaction,
                update_table::<GatewayScopes>()
                    .set(GatewayScopes::last_issued_revision(), publication.revision)
                    .set(GatewayScopes::aggregate_version(), next_version)
                    .set(GatewayScopes::updated_at(), publication.command_issued_at)
                    .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
                    .filter(GatewayScopes::aggregate_version().eq(current.aggregate_version)),
            )
            .await?,
        )?;
    }
    Ok(())
}

fn validate_physical_scope(scope: &GatewayScopeState) -> Result<(), PostgresPersistenceError> {
    if scope.node_id.as_uuid().is_nil()
        || scope
            .installed_revision
            .is_some_and(|revision| revision == 0 || revision > scope.last_issued_revision)
        || if scope.last_issued_revision == 0 {
            scope.aggregate_version != 0 || scope.installed_revision.is_some()
        } else {
            scope.aggregate_version == 0
        }
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored physical Gateway scope is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_dispatch_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "MCP Gateway snapshot dispatch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
