use super::postgres::{
    insert_publication, PostgresEdgeRepository, PublicationRow, PublicationSelection,
};
use super::postgres_mcp_gateway_scope_evidence::{
    insert_scope_evidence, load_scope_statuses, lock_scope_statuses, reconciliation_scope_set,
    reconciliation_scopes,
};
use super::postgres_mcp_gateway_snapshot_cas::{
    advance_physical_scope, lock_credentials, lock_domain_claims, lock_logical_scopes,
    lock_mcp_policies, lock_node_scope_set, lock_ordinary_routes, lock_physical_scope,
    lock_workloads,
};
use super::postgres_schema::{
    GatewayCertificates, GatewayPublications, GatewayScopeMembers, McpGatewaySnapshotPublications,
    McpRoutePolicies, Nodes,
};
use super::postgres_tls::{
    insert_certificate, update_certificate, CertificateRow, CertificateSelection,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::infrastructure::{
    CompiledMcpGatewaySnapshot, IMcpGatewaySnapshotRepository, McpGatewayReconciliationScope,
    McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotScopeStatus,
    McpGatewaySnapshotStageResult, McpGatewaySnapshotStatus, StageMcpGatewaySnapshot,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeCommandId,
    NodeId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, lock_table, select_from, update_table, Database, DecodeError, Expression, FromRow,
    FromValue, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTableLockMode,
    PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

    async fn mcp_gateway_reconciliation_scope_set(
        &self,
        node_id: NodeId,
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<crate::modules::edge::domain::GatewayScope>, RepositoryError> {
        reconciliation_scope_set(&self.executor, node_id, observed_at).await
    }

    async fn mcp_gateway_snapshot_reconciliation_state(
        &self,
        node_id: NodeId,
    ) -> Result<McpGatewaySnapshotReconciliationState, RepositoryError> {
        reconciliation_state(&self.executor, node_id).await
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
                    let (candidate, publication, certificate, event) = stage.into_parts();
                    let scope = candidate.mcp().primary_scope();
                    let organization_id = fetch_optional::<Uuid, _>(
                        transaction,
                        select_from::<Nodes>()
                            .select(Nodes::organization_id())
                            .filter(Nodes::id().eq(publication.node_id.as_uuid()))
                            .for_update(),
                    )
                    .await?
                    .ok_or(RepositoryError::NotFound)?;
                    if organization_id != scope.organization_id.as_uuid() {
                        return Err(RepositoryError::NotFound.into());
                    }

                    lock_logical_scopes(transaction, &candidate).await?;
                    execute(
                        transaction,
                        lock_table::<McpRoutePolicies>(PostgresTableLockMode::Share),
                    )
                    .await?;
                    execute(
                        transaction,
                        lock_table::<GatewayScopeMembers>(PostgresTableLockMode::Share),
                    )
                    .await?;
                    lock_node_scope_set(transaction, &candidate).await?;
                    let physical_scope = lock_physical_scope(transaction, &candidate).await?;
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
                    insert_marker(transaction, &candidate, &publication).await?;
                    insert_scope_evidence(transaction, &candidate, &publication).await?;
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

    async fn stage_managed_gateway_rollout(
        &self,
        stage: StageManagedGatewayRollout,
    ) -> Result<crate::modules::edge::domain::repositories::GatewayRolloutResult, RepositoryError>
    {
        super::postgres_rollouts::stage_managed(&self.executor, stage).await
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
    pub(super) mcp_route_count: u32,
    pub(super) publication_owner: GatewaySnapshotPublicationOwner,
    pub(super) staged_at: DateTime<Utc>,
    pub(super) scope_statuses: Vec<McpGatewaySnapshotScopeStatus>,
}

impl McpGatewaySnapshotMarker {
    pub(super) fn validate_for(
        &self,
        publication: &crate::modules::edge::domain::GatewayPublication,
    ) -> Result<(), String> {
        publication.snapshot()?;
        crate::modules::shared_kernel::domain::Sha256Digest::parse(self.snapshot_digest.clone())?;
        for scope in &self.scope_statuses {
            scope.validate()?;
        }
        let primary = self.scope_statuses.first().ok_or_else(|| {
            "stored MCP Gateway snapshot omitted logical scope evidence".to_string()
        })?;
        let route_count = self.scope_statuses.iter().try_fold(0_u32, |total, scope| {
            total
                .checked_add(scope.mcp_route_count)
                .ok_or_else(|| "stored MCP Gateway snapshot route count overflowed".to_string())
        })?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.gateway_revision == 0
            || self.mcp_route_count > 1_000
            || self
                .scope_statuses
                .windows(2)
                .any(|scopes| scopes[0].gateway_scope_id >= scopes[1].gateway_scope_id)
            || primary.organization_id != self.organization_id
            || primary.project_id != self.project_id
            || primary.environment_id != self.environment_id
            || primary.gateway_scope_id != self.gateway_scope_id
            || self
                .scope_statuses
                .iter()
                .any(|scope| scope.organization_id != self.organization_id)
            || route_count != self.mcp_route_count
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
            mcp_route_count: decode(row, offset + 9)?,
            staged_at: decode(row, offset + 10)?,
        })
    }

    fn marker(
        self,
        scope_statuses: Vec<McpGatewaySnapshotScopeStatus>,
    ) -> Result<McpGatewaySnapshotMarker, RepositoryError> {
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
            mcp_route_count: self.mcp_route_count,
            publication_owner: GatewaySnapshotPublicationOwner::parse(&self.publication_owner)
                .map_err(RepositoryError::Storage)?,
            staged_at: self.staged_at,
            scope_statuses,
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
    fn status(
        self,
        scope_statuses: Vec<McpGatewaySnapshotScopeStatus>,
    ) -> Result<McpGatewaySnapshotStatus, RepositoryError> {
        let publication = self.publication.publication()?;
        let marker = self.marker.marker(scope_statuses)?;
        marker
            .validate_for(&publication)
            .map_err(RepositoryError::Storage)?;
        let status = McpGatewaySnapshotStatus {
            organization_id: marker.organization_id,
            project_id: marker.project_id,
            environment_id: marker.environment_id,
            gateway_scope_id: marker.gateway_scope_id,
            scope_statuses: marker.scope_statuses,
            desired_state_digest: marker.desired_state_digest,
            mcp_route_count: marker.mcp_route_count,
            publication,
            certificate: None,
        };
        status.validate().map_err(RepositoryError::Storage)?;
        Ok(status)
    }
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
    let latest_row = database
        .fetch_optional_as(
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
                .filter(McpGatewaySnapshotPublications::node_id().eq(node_id.as_uuid()))
                .order_by(
                    McpGatewaySnapshotPublications::gateway_revision(),
                    OrderDirection::Desc,
                )
                .limit(1),
        )
        .await
        .map_err(storage)?;
    let mut latest_mcp_snapshot = match latest_row {
        Some(row) => {
            let scope_statuses =
                load_scope_statuses(executor, node_id, row.marker.gateway_revision).await?;
            Some(row.status(scope_statuses)?)
        }
        None => None,
    };
    if let Some(status) = &mut latest_mcp_snapshot {
        status.certificate = database
            .fetch_optional_as(
                select_from::<GatewayCertificates>()
                    .select(CertificateSelection)
                    .filter(GatewayCertificates::node_id().eq(status.publication.node_id.as_uuid()))
                    .filter(GatewayCertificates::gateway_revision().eq(status.publication.revision))
                    .limit(1),
            )
            .await
            .map_err(storage)?
            .map(CertificateRow::certificate)
            .transpose()?;
        status.validate().map_err(RepositoryError::Storage)?;
    }
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

async fn insert_marker(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
) -> Result<GatewayScopeState, PostgresPersistenceError> {
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
    lock_logical_scopes(transaction, candidate).await?;
    let physical_scope = lock_physical_scope(transaction, candidate).await?;
    lock_ordinary_routes(transaction, candidate).await?;
    lock_mcp_policies(transaction, candidate).await?;
    lock_domain_claims(transaction, candidate).await?;
    lock_workloads(transaction, candidate).await?;
    lock_credentials(transaction, candidate).await?;
    Ok(physical_scope)
}

pub(super) async fn persist_managed_composition(
    transaction: &PostgresTransaction,
    composition: &GatewayManagedSnapshotComposition,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let scope = candidate.mcp().primary_scope();
    let mcp_route_count = u32::try_from(
        candidate
            .mcp()
            .projection()
            .map(|projection| projection.projection().routes.len())
            .unwrap_or_default(),
    )
    .map_err(|_| {
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
                    scope.organization_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::project_id(),
                    scope.project_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::environment_id(),
                    scope.environment_id.as_uuid(),
                )
                .value(
                    McpGatewaySnapshotPublications::gateway_scope_id(),
                    scope.id.as_uuid(),
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
    let mut targets = Vec::with_capacity(rows.len());
    for row in rows {
        let scope_statuses = load_scope_statuses(
            executor,
            NodeId::from_uuid(row.marker.node_id),
            row.marker.gateway_revision,
        )
        .await?;
        targets.push(
            row.marker
                .marker(scope_statuses)?
                .dispatch_target(row.publication.publication()?)?,
        );
    }
    Ok(targets)
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
    let Some(row) = row else {
        return Ok(None);
    };
    let scope_statuses = lock_scope_statuses(transaction, node_id, gateway_revision).await?;
    Ok(Some(
        row.marker(scope_statuses)
            .map_err(PostgresPersistenceError::from)?,
    ))
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
