use super::postgres::{
    insert_publication, PostgresEdgeRepository, PublicationRow, PublicationSelection, RouteRow,
    RouteSelection,
};
use super::postgres_schema::{
    DomainClaims, GatewayCertificates, GatewayPublications, GatewayRouteProjections,
    GatewayRouteScopes, GatewayScopeMembers, GatewayScopes, McpCredentials,
    McpGatewaySnapshotPublicationScopes, McpGatewaySnapshotPublications, McpRoutePolicies, Nodes,
    Routes, Workloads,
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
    CompiledMcpGatewaySnapshot, IMcpGatewaySnapshotRepository, McpGatewayReconciliationScope,
    McpGatewaySnapshotDispatchTarget, McpGatewaySnapshotInputs,
    McpGatewaySnapshotReconciliationState, McpGatewaySnapshotScopeStatus,
    McpGatewaySnapshotStageResult, McpGatewaySnapshotStatus, StageMcpGatewaySnapshot,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayCertificateId, GatewayScopeId, NodeCommandId,
    NodeId, OrganizationId, ProjectId, RepositoryError, WorkloadId, WorkloadRevisionId,
};
use a3s_orm::expression::{exists, not, Selection};
use a3s_orm::{
    insert_into, lock_table, select_from, sql_query, update_table, Database, DecodeError,
    Expression, FromRow, FromValue, OrderDirection, PostgresDialect, PostgresExecutor,
    PostgresTableLockMode, PostgresTransaction, Row,
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

                    lock_ordinary_routes(transaction, &candidate).await?;
                    lock_mcp_policies(transaction, &candidate).await?;
                    lock_domain_claims(transaction, &candidate).await?;
                    lock_workloads(transaction, &candidate).await?;
                    lock_credentials(transaction, &candidate).await?;

                    insert_publication(transaction, &publication).await?;
                    if let Some(certificate) = &certificate {
                        insert_certificate(transaction, certificate).await?;
                    }
                    insert_marker(transaction, &candidate, &publication).await?;
                    insert_scope_evidence(transaction, &candidate, &publication).await?;
                    advance_physical_scope(transaction, &physical_scope, &publication).await?;
                    store_outbox(transaction, &event).await?;
                    Ok(McpGatewaySnapshotStageResult {
                        publication,
                        certificate,
                    })
                })
            })
            .await
            .map_err(transaction_error)
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
            staged_at: self.staged_at,
            scope_statuses,
        })
    }
}

struct McpGatewaySnapshotScopeStatusRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_scope_id: Uuid,
    scope_aggregate_version: u64,
    membership_generation: u64,
    receiving_member: bool,
    mcp_route_count: u32,
}

struct McpGatewaySnapshotScopeStatusSelection;

impl Selection for McpGatewaySnapshotScopeStatusSelection {
    type Output = McpGatewaySnapshotScopeStatusRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            McpGatewaySnapshotPublicationScopes::organization_id().expression(),
            McpGatewaySnapshotPublicationScopes::project_id().expression(),
            McpGatewaySnapshotPublicationScopes::environment_id().expression(),
            McpGatewaySnapshotPublicationScopes::gateway_scope_id().expression(),
            McpGatewaySnapshotPublicationScopes::scope_aggregate_version().expression(),
            McpGatewaySnapshotPublicationScopes::membership_generation().expression(),
            McpGatewaySnapshotPublicationScopes::receiving_member().expression(),
            McpGatewaySnapshotPublicationScopes::mcp_route_count().expression(),
        ]
    }
}

impl FromRow for McpGatewaySnapshotScopeStatusRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            gateway_scope_id: decode(row, 3)?,
            scope_aggregate_version: decode(row, 4)?,
            membership_generation: decode(row, 5)?,
            receiving_member: decode(row, 6)?,
            mcp_route_count: decode(row, 7)?,
        })
    }
}

impl McpGatewaySnapshotScopeStatusRow {
    fn status(self) -> Result<McpGatewaySnapshotScopeStatus, RepositoryError> {
        let status = McpGatewaySnapshotScopeStatus {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            gateway_scope_id: GatewayScopeId::from_uuid(self.gateway_scope_id),
            scope_aggregate_version: self.scope_aggregate_version,
            membership_generation: self.membership_generation,
            receiving_member: self.receiving_member,
            mcp_route_count: self.mcp_route_count,
        };
        status.validate().map_err(RepositoryError::Storage)?;
        Ok(status)
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
    let mut query = sql_query::<(Uuid, Uuid)>(
        "select scope.organization_id, scope.id \
         from gateway_route_scopes scope \
         where (exists (select 1 from mcp_route_policies policy \
                        where policy.gateway_scope_id = scope.id \
                          and policy.expires_at > ",
    )
    .bind(observed_at)
    .append(
        ") or exists (select 1 \
                       from mcp_gateway_snapshot_publication_scopes evidence \
                       where evidence.gateway_scope_id = scope.id \
                         and evidence.mcp_route_count > 0 \
                         and not exists (select 1 \
                                         from mcp_gateway_snapshot_publication_scopes newer \
                                         where newer.gateway_scope_id = evidence.gateway_scope_id \
                                           and newer.node_id = evidence.node_id \
                                           and newer.gateway_revision > evidence.gateway_revision)))",
    );
    if let Some(after_gateway_scope_id) = after_gateway_scope_id {
        query = query
            .append(" and scope.id > ")
            .bind(after_gateway_scope_id.as_uuid());
    }
    query = query.append(" order by scope.id asc limit ").bind(limit);
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(storage)?
        .rows;
    let mut scopes = Vec::with_capacity(identities.len());
    for (organization_id, scope_id) in identities {
        let scope = postgres_gateway_scopes::find(
            executor,
            OrganizationId::from_uuid(organization_id),
            GatewayScopeId::from_uuid(scope_id),
        )
        .await?;
        let node_ids = reconciliation_nodes_for_scope(executor, scope.id, observed_at).await?;
        if !node_ids.is_empty() {
            let reconciliation_scope = McpGatewayReconciliationScope { scope, node_ids };
            reconciliation_scope
                .validate()
                .map_err(RepositoryError::Storage)?;
            scopes.push(reconciliation_scope);
        }
    }
    Ok(scopes)
}

async fn reconciliation_nodes_for_scope(
    executor: &PostgresExecutor,
    gateway_scope_id: GatewayScopeId,
    observed_at: DateTime<Utc>,
) -> Result<Vec<NodeId>, RepositoryError> {
    let database = Database::new(PostgresDialect, executor.clone());
    let has_active_policy = database
        .fetch_optional_as(
            select_from::<McpRoutePolicies>()
                .select(McpRoutePolicies::id())
                .filter(McpRoutePolicies::gateway_scope_id().eq(gateway_scope_id.as_uuid()))
                .filter(McpRoutePolicies::expires_at().gt(observed_at))
                .limit(1),
        )
        .await
        .map_err(storage)?
        .is_some();
    let mut node_ids = BTreeSet::new();
    if has_active_policy {
        node_ids.extend(
            database
                .fetch_all_as(
                    select_from::<GatewayScopeMembers>()
                        .select(GatewayScopeMembers::node_id())
                        .filter(
                            GatewayScopeMembers::gateway_scope_id().eq(gateway_scope_id.as_uuid()),
                        )
                        .order_by(GatewayScopeMembers::node_id(), OrderDirection::Asc),
                )
                .await
                .map_err(storage)?
                .rows
                .into_iter()
                .map(NodeId::from_uuid),
        );
    }
    let prior_nodes = database
        .fetch_all_as(
            sql_query::<Uuid>(
                "select evidence.node_id \
                 from mcp_gateway_snapshot_publication_scopes evidence \
                 where evidence.gateway_scope_id = ",
            )
            .bind(gateway_scope_id.as_uuid())
            .append(
                " and evidence.mcp_route_count > 0 \
                  and not exists (select 1 \
                                  from mcp_gateway_snapshot_publication_scopes newer \
                                  where newer.gateway_scope_id = evidence.gateway_scope_id \
                                    and newer.node_id = evidence.node_id \
                                    and newer.gateway_revision > evidence.gateway_revision) \
                  order by evidence.node_id asc limit 10001",
            ),
        )
        .await
        .map_err(storage)?
        .rows;
    if prior_nodes.len() > 10_000 {
        return Err(RepositoryError::Storage(
            "MCP Gateway reconciliation scope exceeded the physical node bound".into(),
        ));
    }
    node_ids.extend(prior_nodes.into_iter().map(NodeId::from_uuid));
    Ok(node_ids.into_iter().collect())
}

async fn reconciliation_scope_set(
    executor: &PostgresExecutor,
    node_id: NodeId,
    observed_at: DateTime<Utc>,
) -> Result<Vec<crate::modules::edge::domain::GatewayScope>, RepositoryError> {
    if node_id.as_uuid().is_nil() {
        return Err(RepositoryError::Conflict(
            "MCP Gateway reconciliation node is invalid".into(),
        ));
    }
    let observed_at = canonical_timestamp(observed_at);
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(observed_at)),
    );
    let database = Database::new(PostgresDialect, executor.clone());
    let current = database
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
    let prior = database
        .fetch_all_as(
            sql_query::<(Uuid, Uuid)>(
                "select evidence.organization_id, evidence.gateway_scope_id \
                 from mcp_gateway_snapshot_publication_scopes evidence \
                 where evidence.node_id = ",
            )
            .bind(node_id.as_uuid())
            .append(
                " and evidence.mcp_route_count > 0 \
                  and not exists (select 1 \
                                  from mcp_gateway_snapshot_publication_scopes newer \
                                  where newer.node_id = evidence.node_id \
                                    and newer.gateway_scope_id = evidence.gateway_scope_id \
                                    and newer.gateway_revision > evidence.gateway_revision) \
                  order by evidence.gateway_scope_id asc limit 1001",
            ),
        )
        .await
        .map_err(storage)?
        .rows;
    if current.len() > 1_000 || prior.len() > 1_000 {
        return Err(RepositoryError::Storage(
            "MCP Gateway reconciliation exceeded the logical scope bound".into(),
        ));
    }
    let mut identities = BTreeMap::<GatewayScopeId, OrganizationId>::new();
    for (organization_id, gateway_scope_id) in current.into_iter().chain(prior) {
        let gateway_scope_id = GatewayScopeId::from_uuid(gateway_scope_id);
        let organization_id = OrganizationId::from_uuid(organization_id);
        if identities
            .insert(gateway_scope_id, organization_id)
            .is_some_and(|existing| existing != organization_id)
        {
            return Err(RepositoryError::Storage(
                "MCP Gateway logical scope identity crossed organizations".into(),
            ));
        }
    }
    if identities.len() > 1_000 {
        return Err(RepositoryError::Storage(
            "MCP Gateway reconciliation exceeded the logical scope bound".into(),
        ));
    }
    let mut scopes = Vec::with_capacity(identities.len());
    for (gateway_scope_id, organization_id) in identities {
        scopes.push(
            postgres_gateway_scopes::find(executor, organization_id, gateway_scope_id).await?,
        );
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

async fn load_scope_statuses(
    executor: &PostgresExecutor,
    node_id: NodeId,
    gateway_revision: u64,
) -> Result<Vec<McpGatewaySnapshotScopeStatus>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<McpGatewaySnapshotPublicationScopes>()
                .select(McpGatewaySnapshotScopeStatusSelection)
                .filter(McpGatewaySnapshotPublicationScopes::node_id().eq(node_id.as_uuid()))
                .filter(
                    McpGatewaySnapshotPublicationScopes::gateway_revision().eq(gateway_revision),
                )
                .order_by(
                    McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                    OrderDirection::Asc,
                ),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(McpGatewaySnapshotScopeStatusRow::status)
        .collect()
}

async fn insert_marker(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
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
                    McpGatewaySnapshotPublications::staged_at(),
                    publication.command_issued_at,
                ),
        )
        .await?,
    )?;
    Ok(())
}

async fn insert_scope_evidence(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let mut total_route_count = 0_u32;
    for planned in candidate.mcp().scope_sets() {
        let scope = planned.scope();
        let mcp_route_count = u32::try_from(
            planned
                .projection()
                .map(|projection| projection.projection().routes.len())
                .unwrap_or_default(),
        )
        .map_err(|_| {
            PostgresPersistenceError::Invariant(
                "MCP Gateway logical scope route count exceeds durable bounds".into(),
            )
        })?;
        total_route_count = total_route_count
            .checked_add(mcp_route_count)
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "MCP Gateway node-wide route count overflowed".into(),
                )
            })?;
        require_one_row(
            "MCP Gateway snapshot logical scope evidence",
            execute(
                transaction,
                insert_into::<McpGatewaySnapshotPublicationScopes>()
                    .value(
                        McpGatewaySnapshotPublicationScopes::organization_id(),
                        scope.organization_id.as_uuid(),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::project_id(),
                        scope.project_id.as_uuid(),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::environment_id(),
                        scope.environment_id.as_uuid(),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                        scope.id.as_uuid(),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::node_id(),
                        publication.node_id.as_uuid(),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::gateway_revision(),
                        publication.revision,
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::scope_aggregate_version(),
                        scope.aggregate_version,
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::membership_generation(),
                        scope.membership_generation,
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::receiving_member(),
                        scope.contains_member(publication.node_id),
                    )
                    .value(
                        McpGatewaySnapshotPublicationScopes::mcp_route_count(),
                        mcp_route_count,
                    ),
            )
            .await?,
        )?;
    }
    let expected_route_count = u32::try_from(
        candidate
            .mcp()
            .projection()
            .map(|projection| projection.projection().routes.len())
            .unwrap_or_default(),
    )
    .map_err(|_| {
        PostgresPersistenceError::Invariant(
            "MCP Gateway complete route count exceeds durable bounds".into(),
        )
    })?;
    if total_route_count != expected_route_count {
        return Err(PostgresPersistenceError::Invariant(
            "MCP Gateway logical scope route counts do not cover the complete projection".into(),
        ));
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

async fn lock_scope_statuses(
    transaction: &PostgresTransaction,
    node_id: Uuid,
    gateway_revision: u64,
) -> Result<Vec<McpGatewaySnapshotScopeStatus>, PostgresPersistenceError> {
    fetch_all::<McpGatewaySnapshotScopeStatusRow, _>(
        transaction,
        select_from::<McpGatewaySnapshotPublicationScopes>()
            .select(McpGatewaySnapshotScopeStatusSelection)
            .filter(McpGatewaySnapshotPublicationScopes::node_id().eq(node_id))
            .filter(McpGatewaySnapshotPublicationScopes::gateway_revision().eq(gateway_revision))
            .order_by(
                McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                OrderDirection::Asc,
            )
            .for_update(),
    )
    .await?
    .into_iter()
    .map(McpGatewaySnapshotScopeStatusRow::status)
    .collect::<Result<Vec<_>, _>>()
    .map_err(PostgresPersistenceError::from)
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
    for planned in candidate.mcp().scope_sets() {
        lock_logical_scope(transaction, planned.scope()).await?;
    }
    Ok(())
}

async fn lock_logical_scope(
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

async fn lock_node_scope_set(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let node_id = candidate.physical_scope().node_id;
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(candidate.mcp().observed_at())),
    );
    let current = fetch_all::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteScopes>()
            .inner_join::<GatewayScopeMembers>(
                GatewayRouteScopes::id().eq_column(GatewayScopeMembers::gateway_scope_id()),
            )
            .select(GatewayRouteScopes::id())
            .filter(GatewayScopeMembers::node_id().eq(node_id.as_uuid()))
            .filter(active_policy)
            .order_by(GatewayRouteScopes::id(), OrderDirection::Asc)
            .limit(1_001),
    )
    .await?;
    let prior = fetch_all::<Uuid, _>(
        transaction,
        sql_query::<Uuid>(
            "select evidence.gateway_scope_id \
             from mcp_gateway_snapshot_publication_scopes evidence \
             where evidence.node_id = ",
        )
        .bind(node_id.as_uuid())
        .append(
            " and evidence.mcp_route_count > 0 \
              and not exists (select 1 \
                              from mcp_gateway_snapshot_publication_scopes newer \
                              where newer.node_id = evidence.node_id \
                                and newer.gateway_scope_id = evidence.gateway_scope_id \
                                and newer.gateway_revision > evidence.gateway_revision) \
              order by evidence.gateway_scope_id asc limit 1001 for update",
        ),
    )
    .await?;
    if current.len() > 1_000 || prior.len() > 1_000 {
        return Err(RepositoryError::Conflict(
            "MCP Gateway node-wide logical scope set exceeds its bound".into(),
        )
        .into());
    }
    let actual = current
        .into_iter()
        .chain(prior)
        .map(GatewayScopeId::from_uuid)
        .collect::<BTreeSet<_>>();
    let expected = candidate
        .mcp()
        .scope_sets()
        .iter()
        .map(|planned| planned.scope().id)
        .collect::<BTreeSet<_>>();
    if actual != expected || expected.len() != candidate.mcp().scope_sets().len() {
        return Err(RepositoryError::Conflict(
            "node-wide MCP Gateway logical scope set changed while planning the snapshot".into(),
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
    for planned in candidate.mcp().scope_sets() {
        if planned
            .scope()
            .contains_member(candidate.physical_scope().node_id)
        {
            lock_mcp_scope_policies(transaction, candidate, planned).await?;
        } else if planned.projection().is_some()
            || !planned.route_versions().is_empty()
            || !planned.credential_authority_versions().is_empty()
            || !planned.ingress_routes().is_empty()
        {
            return Err(RepositoryError::Conflict(
                "departed MCP Gateway scope retained node-local projection evidence".into(),
            )
            .into());
        }
    }
    Ok(())
}

async fn lock_mcp_scope_policies(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
    planned: &crate::modules::edge::infrastructure::PlannedMcpGatewayProjectionSet,
) -> Result<(), PostgresPersistenceError> {
    let scope = planned.scope();
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
    let expected = planned.route_versions();
    if rows.len() != expected.len()
        || rows.iter().zip(expected).any(
            |((route_id, revision, digest, workload_id, domain_claim_id, updated_at), expected)| {
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
    Ok(())
}

async fn lock_domain_claims(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mut mcp_claims = BTreeMap::new();
    for planned in candidate.mcp().scope_sets() {
        let scope = planned.scope();
        for version in planned.route_versions() {
            let tenant = (
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
            );
            if mcp_claims
                .insert(version.domain_claim_id(), tenant)
                .is_some_and(|existing| existing != tenant)
            {
                return Err(RepositoryError::Conflict(
                    "MCP snapshot DomainClaim crossed logical scope tenants".into(),
                )
                .into());
            }
        }
    }
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
        let scope = candidate.mcp().primary_scope();
        let mcp_tenant = mcp_claims.get(&expected.domain_claim_id());
        if organization_id != scope.organization_id.as_uuid()
            || mcp_tenant.is_some_and(|(organization, project, environment)| {
                organization_id != organization.as_uuid()
                    || project_id != project.as_uuid()
                    || environment_id != environment.as_uuid()
            })
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
            u64,
            WorkloadRevisionId,
            OrganizationId,
            ProjectId,
            EnvironmentId,
        ),
    >::new();
    for planned in candidate.mcp().scope_sets() {
        let scope = planned.scope();
        for version in planned.route_versions() {
            let authority = (
                version.workload_aggregate_version(),
                version.active_revision_id(),
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
            );
            match expected.get(&version.workload_id()) {
                Some(value) if *value != authority => {
                    return Err(RepositoryError::Conflict(
                        "MCP snapshot observed conflicting authority for one Workload".into(),
                    )
                    .into())
                }
                Some(_) => {}
                None => {
                    expected.insert(version.workload_id(), authority);
                }
            }
        }
    }
    for (
        workload_id,
        (aggregate_version, active_revision_id, organization_id, project_id, environment_id),
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

async fn lock_credentials(
    transaction: &PostgresTransaction,
    candidate: &CompiledMcpGatewaySnapshot,
) -> Result<(), PostgresPersistenceError> {
    let mut expected_credentials = BTreeMap::new();
    for planned in candidate.mcp().scope_sets() {
        let scope = planned.scope();
        for expected in planned.credential_authority_versions() {
            let authority = (
                *expected,
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
            );
            if expected_credentials
                .insert(expected.credential_id(), authority)
                .is_some_and(|existing| existing != authority)
            {
                return Err(RepositoryError::Conflict(
                    "MCP snapshot credential authority crossed logical scope tenants".into(),
                )
                .into());
            }
        }
    }
    for (credential_id, (expected, organization_id, project_id, environment_id)) in
        expected_credentials
    {
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
        if row.0 != organization_id.as_uuid()
            || row.1 != project_id.as_uuid()
            || row.2 != environment_id.as_uuid()
            || row.3 != expected.generation()
            || row.4 != expected.aggregate_version()
            || (row.5 > candidate.mcp().observed_at() && row.6.is_none())
                != expected.active_at_observed_at()
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
