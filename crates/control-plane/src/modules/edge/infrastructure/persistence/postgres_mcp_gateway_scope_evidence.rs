use super::postgres_gateway_scopes;
use super::postgres_schema::{
    GatewayRouteScopes, GatewayScopeMembers, McpGatewaySnapshotPublicationScopes, McpRoutePolicies,
};
use crate::infrastructure::{execute, fetch_all, require_one_row, PostgresPersistenceError};
use crate::modules::edge::infrastructure::{
    CompiledMcpGatewaySnapshot, McpGatewayReconciliationScope, McpGatewaySnapshotScopeStatus,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, NodeId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_orm::expression::{exists, not, Selection};
use a3s_orm::{
    insert_into, orm_table, select_from, select_from_as, Database, DecodeError, Expression,
    FromRow, FromValue, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
    Row,
};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

orm_table! {
    struct NewerMcpGatewaySnapshotPublicationScopes => "newer_mcp_gateway_snapshot_publication_scope" {
        gateway_scope_id: Uuid => "gateway_scope_id",
        node_id: Uuid => "node_id",
        gateway_revision: u64 => "gateway_revision",
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

pub(super) async fn reconciliation_scopes(
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
    let result_limit = limit;
    let query_limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("MCP Gateway reconciliation scope limit is invalid".into())
    })?;
    let active_policy = exists(
        select_from::<McpRoutePolicies>()
            .select(McpRoutePolicies::id())
            .filter(McpRoutePolicies::gateway_scope_id().eq_column(GatewayRouteScopes::id()))
            .filter(McpRoutePolicies::expires_at().gt(observed_at)),
    );
    let mut active_query = select_from::<GatewayRouteScopes>()
        .select((
            GatewayRouteScopes::organization_id(),
            GatewayRouteScopes::id(),
        ))
        .filter(active_policy);
    let mut prior_query = select_from::<McpGatewaySnapshotPublicationScopes>()
        .select((
            McpGatewaySnapshotPublicationScopes::organization_id(),
            McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
        ))
        .distinct()
        .filter(McpGatewaySnapshotPublicationScopes::mcp_route_count().gt(0_u32))
        .filter(no_newer_scope_evidence());
    if let Some(after_gateway_scope_id) = after_gateway_scope_id {
        active_query =
            active_query.filter(GatewayRouteScopes::id().gt(after_gateway_scope_id.as_uuid()));
        prior_query = prior_query.filter(
            McpGatewaySnapshotPublicationScopes::gateway_scope_id()
                .gt(after_gateway_scope_id.as_uuid()),
        );
    }
    let database = Database::new(PostgresDialect, executor.clone());
    let active_identities = database
        .fetch_all_as(
            active_query
                .order_by(GatewayRouteScopes::id(), OrderDirection::Asc)
                .limit(query_limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let prior_identities = database
        .fetch_all_as(
            prior_query
                .order_by(
                    McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                    OrderDirection::Asc,
                )
                .limit(query_limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut identities = BTreeMap::<GatewayScopeId, OrganizationId>::new();
    for (organization_id, scope_id) in active_identities.into_iter().chain(prior_identities) {
        let scope_id = GatewayScopeId::from_uuid(scope_id);
        let organization_id = OrganizationId::from_uuid(organization_id);
        if identities
            .insert(scope_id, organization_id)
            .is_some_and(|existing| existing != organization_id)
        {
            return Err(RepositoryError::Storage(
                "MCP Gateway reconciliation scope identity crossed organizations".into(),
            ));
        }
    }
    let mut scopes = Vec::with_capacity(identities.len().min(result_limit));
    for (scope_id, organization_id) in identities.into_iter().take(result_limit) {
        let scope = postgres_gateway_scopes::find(executor, organization_id, scope_id).await?;
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
            select_from::<McpGatewaySnapshotPublicationScopes>()
                .select(McpGatewaySnapshotPublicationScopes::node_id())
                .distinct()
                .filter(
                    McpGatewaySnapshotPublicationScopes::gateway_scope_id()
                        .eq(gateway_scope_id.as_uuid()),
                )
                .filter(McpGatewaySnapshotPublicationScopes::mcp_route_count().gt(0_u32))
                .filter(no_newer_scope_evidence())
                .order_by(
                    McpGatewaySnapshotPublicationScopes::node_id(),
                    OrderDirection::Asc,
                )
                .limit(10_001),
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

pub(super) async fn reconciliation_scope_set(
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
            select_from::<McpGatewaySnapshotPublicationScopes>()
                .select((
                    McpGatewaySnapshotPublicationScopes::organization_id(),
                    McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                ))
                .distinct()
                .filter(McpGatewaySnapshotPublicationScopes::node_id().eq(node_id.as_uuid()))
                .filter(McpGatewaySnapshotPublicationScopes::mcp_route_count().gt(0_u32))
                .filter(no_newer_scope_evidence())
                .order_by(
                    McpGatewaySnapshotPublicationScopes::gateway_scope_id(),
                    OrderDirection::Asc,
                )
                .limit(1_001),
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

pub(super) async fn load_scope_statuses(
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

pub(super) async fn insert_scope_evidence(
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

pub(super) async fn lock_scope_statuses(
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

fn no_newer_scope_evidence() -> Expression {
    not(exists(
        select_from_as::<
            McpGatewaySnapshotPublicationScopes,
            NewerMcpGatewaySnapshotPublicationScopes,
        >()
        .select(NewerMcpGatewaySnapshotPublicationScopes::gateway_revision())
        .filter(
            NewerMcpGatewaySnapshotPublicationScopes::gateway_scope_id()
                .eq_column(McpGatewaySnapshotPublicationScopes::gateway_scope_id()),
        )
        .filter(
            NewerMcpGatewaySnapshotPublicationScopes::node_id()
                .eq_column(McpGatewaySnapshotPublicationScopes::node_id()),
        )
        .filter(
            McpGatewaySnapshotPublicationScopes::gateway_revision()
                .lt_column(NewerMcpGatewaySnapshotPublicationScopes::gateway_revision()),
        ),
    ))
}
