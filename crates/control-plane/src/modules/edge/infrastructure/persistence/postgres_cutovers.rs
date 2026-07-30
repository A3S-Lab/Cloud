use super::postgres::{insert_publication, RouteRow, RouteSelection};
use super::postgres_gateway_scopes;
use super::postgres_tls::insert_certificate;
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_idempotency, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    GatewayRouteCutoverResult, StageGatewayRouteCutover,
};
use crate::modules::edge::domain::{
    GatewayRouteCutover, GatewayRouteCutoverState, GatewayScopeState, Route, RouteState,
};
use crate::modules::edge::infrastructure::{
    GatewayManagedSnapshotComposition, StageManagedGatewayRouteCutover,
};
use crate::modules::shared_kernel::domain::{
    DeploymentId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId, RepositoryError,
    WorkloadId, WorkloadRevisionId,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    insert_into, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::postgres_schema::{
    GatewayPublications, GatewayRouteCutovers, GatewayScopes, Nodes, Routes,
};

struct CutoverRow {
    deployment_id: Uuid,
    organization_id: Uuid,
    workload_id: Uuid,
    previous_revision_id: Uuid,
    candidate_revision_id: Uuid,
    previous_generation: u64,
    candidate_generation: u64,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    gateway_certificate_id: Uuid,
    snapshot_digest: String,
    snapshot_expires_at: DateTime<Utc>,
    routes: serde_json::Value,
    state: String,
    failure: Option<String>,
    staged_at: DateTime<Utc>,
    acknowledged_at: Option<DateTime<Utc>>,
}

struct CutoverSelection;

impl Selection for CutoverSelection {
    type Output = CutoverRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRouteCutovers::deployment_id().expression(),
            GatewayRouteCutovers::organization_id().expression(),
            GatewayRouteCutovers::workload_id().expression(),
            GatewayRouteCutovers::previous_revision_id().expression(),
            GatewayRouteCutovers::candidate_revision_id().expression(),
            GatewayRouteCutovers::previous_generation().expression(),
            GatewayRouteCutovers::candidate_generation().expression(),
            GatewayRouteCutovers::node_id().expression(),
            GatewayRouteCutovers::gateway_revision().expression(),
            GatewayRouteCutovers::gateway_command_id().expression(),
            GatewayRouteCutovers::gateway_certificate_id().expression(),
            GatewayRouteCutovers::snapshot_digest().expression(),
            GatewayRouteCutovers::snapshot_expires_at().expression(),
            GatewayRouteCutovers::routes().expression(),
            GatewayRouteCutovers::state().expression(),
            GatewayRouteCutovers::failure().expression(),
            GatewayRouteCutovers::staged_at().expression(),
            GatewayRouteCutovers::acknowledged_at().expression(),
        ]
    }
}

impl FromRow for CutoverRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            deployment_id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            workload_id: decode(row, 2)?,
            previous_revision_id: decode(row, 3)?,
            candidate_revision_id: decode(row, 4)?,
            previous_generation: decode(row, 5)?,
            candidate_generation: decode(row, 6)?,
            node_id: decode(row, 7)?,
            gateway_revision: decode(row, 8)?,
            gateway_command_id: decode(row, 9)?,
            gateway_certificate_id: decode(row, 10)?,
            snapshot_digest: decode(row, 11)?,
            snapshot_expires_at: decode(row, 12)?,
            routes: decode(row, 13)?,
            state: decode(row, 14)?,
            failure: decode(row, 15)?,
            staged_at: decode(row, 16)?,
            acknowledged_at: decode(row, 17)?,
        })
    }
}

impl CutoverRow {
    fn cutover(self) -> Result<GatewayRouteCutover, RepositoryError> {
        let cutover = GatewayRouteCutover {
            deployment_id: DeploymentId::from_uuid(self.deployment_id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            workload_id: WorkloadId::from_uuid(self.workload_id),
            previous_revision_id: WorkloadRevisionId::from_uuid(self.previous_revision_id),
            candidate_revision_id: WorkloadRevisionId::from_uuid(self.candidate_revision_id),
            previous_generation: self.previous_generation,
            candidate_generation: self.candidate_generation,
            node_id: NodeId::from_uuid(self.node_id),
            gateway_revision: self.gateway_revision,
            gateway_command_id: NodeCommandId::from_uuid(self.gateway_command_id),
            gateway_certificate_id: GatewayCertificateId::from_uuid(self.gateway_certificate_id),
            snapshot_digest: self.snapshot_digest,
            snapshot_expires_at: self.snapshot_expires_at,
            routes: serde_json::from_value(self.routes)
                .map_err(|error| stored("route cutover routes")(error.to_string()))?,
            state: GatewayRouteCutoverState::parse(&self.state)
                .map_err(stored("route cutover state"))?,
            failure: self.failure,
            staged_at: self.staged_at,
            acknowledged_at: self.acknowledged_at,
        };
        cutover
            .validate()
            .map_err(stored("route cutover projection"))?;
        Ok(cutover)
    }
}

pub(super) async fn replay(
    executor: &PostgresExecutor,
    idempotency: &crate::modules::shared_kernel::domain::IdempotencyRequest,
) -> Result<Option<GatewayRouteCutoverResult>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(mut replay) =
                    idempotency_replay::<GatewayRouteCutoverResult>(transaction, &idempotency)
                        .await?
                else {
                    return Ok(None);
                };
                replay.value.replayed = true;
                Ok(Some(replay.value))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn stage(
    executor: &PostgresExecutor,
    bundle: StageGatewayRouteCutover,
) -> Result<GatewayRouteCutoverResult, RepositoryError> {
    stage_impl(executor, bundle, None).await
}

pub(super) async fn stage_managed(
    executor: &PostgresExecutor,
    stage: StageManagedGatewayRouteCutover,
) -> Result<GatewayRouteCutoverResult, RepositoryError> {
    let (bundle, composition) = stage.into_parts();
    stage_impl(executor, bundle, Some(composition)).await
}

async fn stage_impl(
    executor: &PostgresExecutor,
    bundle: StageGatewayRouteCutover,
    composition: Option<GatewayManagedSnapshotComposition>,
) -> Result<GatewayRouteCutoverResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    if let Some(composition) = &composition {
        composition
            .validate_for(&bundle.publication)
            .map_err(RepositoryError::Conflict)?;
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(mut replay) = idempotency_replay::<GatewayRouteCutoverResult>(
                    transaction,
                    &bundle.idempotency,
                )
                .await?
                {
                    replay.value.replayed = true;
                    return Ok(replay.value);
                }
                postgres_gateway_scopes::validate_cutover_bindings(
                    transaction,
                    &bundle.cutover.routes,
                )
                .await?;
                let current = match &composition {
                    Some(composition) => {
                        super::postgres_mcp_gateway_snapshots::lock_managed_composition(
                            transaction,
                            composition,
                        )
                        .await?
                    }
                    None => {
                        let organization_id = fetch_optional::<Uuid, _>(
                            transaction,
                            select_from::<Nodes>()
                                .select(Nodes::organization_id())
                                .filter(Nodes::id().eq(bundle.publication.node_id.as_uuid()))
                                .for_update(),
                        )
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                        if organization_id != bundle.cutover.organization_id.as_uuid() {
                            return Err(RepositoryError::NotFound.into());
                        }
                        let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
                            transaction,
                            select_from::<GatewayScopes>()
                                .select((
                                    GatewayScopes::last_issued_revision(),
                                    GatewayScopes::installed_revision(),
                                    GatewayScopes::aggregate_version(),
                                ))
                                .filter(
                                    GatewayScopes::node_id()
                                        .eq(bundle.publication.node_id.as_uuid()),
                                )
                                .for_update(),
                        )
                        .await?;
                        match scope {
                            Some((last, installed, version)) => {
                                validate_scope(last, installed, version)?;
                                GatewayScopeState {
                                    node_id: bundle.publication.node_id,
                                    last_issued_revision: last,
                                    installed_revision: installed,
                                    aggregate_version: version,
                                }
                            }
                            None => GatewayScopeState::empty(bundle.publication.node_id),
                        }
                    }
                };
                if current.aggregate_version != bundle.expected_scope_version {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed while compiling the route cutover snapshot".into(),
                    )
                    .into());
                }
                let pending = fetch_optional::<u64, _>(
                    transaction,
                    select_from::<GatewayPublications>()
                        .select(GatewayPublications::revision())
                        .filter(
                            GatewayPublications::node_id().eq(bundle.publication.node_id.as_uuid()),
                        )
                        .filter(GatewayPublications::state().eq("pending"))
                        .for_update(),
                )
                .await?;
                if pending.is_some() {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope already has a pending complete snapshot".into(),
                    )
                    .into());
                }
                if bundle.publication.revision
                    != current.next_revision().map_err(RepositoryError::Conflict)?
                    || bundle.publication.expected_revision != current.installed_revision
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway route cutover does not advance the authoritative scope revision"
                            .into(),
                    )
                    .into());
                }
                let active_rows = fetch_all::<RouteRow, _>(
                    transaction,
                    select_from::<Routes>()
                        .select(RouteSelection)
                        .filter(
                            Routes::organization_id().eq(bundle.cutover.organization_id.as_uuid()),
                        )
                        .filter(Routes::workload_id().eq(bundle.cutover.workload_id.as_uuid()))
                        .filter(Routes::state().eq("active"))
                        .order_by(Routes::id(), OrderDirection::Asc)
                        .for_update(),
                )
                .await?;
                let active_routes = active_rows
                    .into_iter()
                    .map(RouteRow::route)
                    .collect::<Result<Vec<_>, _>>()?;
                validate_pending_routes(&active_routes, &bundle.cutover)?;

                insert_publication(transaction, &bundle.publication).await?;
                insert_certificate(transaction, &bundle.certificate).await?;
                insert_cutover(transaction, &bundle.cutover).await?;
                if let Some(composition) = &composition {
                    super::postgres_mcp_gateway_snapshots::persist_managed_composition(
                        transaction,
                        composition,
                        &bundle.publication,
                    )
                    .await?;
                }
                if current.aggregate_version == 0 {
                    require_one_row(
                        "Gateway scope",
                        execute(
                            transaction,
                            insert_into::<GatewayScopes>()
                                .value(
                                    GatewayScopes::node_id(),
                                    bundle.publication.node_id.as_uuid(),
                                )
                                .value(
                                    GatewayScopes::last_issued_revision(),
                                    bundle.publication.revision,
                                )
                                .value(
                                    GatewayScopes::installed_revision(),
                                    current.installed_revision,
                                )
                                .value(GatewayScopes::aggregate_version(), 1_u64)
                                .value(
                                    GatewayScopes::updated_at(),
                                    bundle.publication.command_issued_at,
                                ),
                        )
                        .await?,
                    )?;
                } else {
                    let next_version =
                        current.aggregate_version.checked_add(1).ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Gateway scope aggregate version overflowed".into(),
                            )
                        })?;
                    require_one_row(
                        "Gateway scope",
                        execute(
                            transaction,
                            update_table::<GatewayScopes>()
                                .set(
                                    GatewayScopes::last_issued_revision(),
                                    bundle.publication.revision,
                                )
                                .set(GatewayScopes::aggregate_version(), next_version)
                                .set(
                                    GatewayScopes::updated_at(),
                                    bundle.publication.command_issued_at,
                                )
                                .filter(
                                    GatewayScopes::node_id()
                                        .eq(bundle.publication.node_id.as_uuid()),
                                )
                                .filter(
                                    GatewayScopes::aggregate_version()
                                        .eq(current.aggregate_version),
                                ),
                        )
                        .await?,
                    )?;
                }
                let result = GatewayRouteCutoverResult {
                    cutover: bundle.cutover,
                    certificate: bundle.certificate,
                    publication: bundle.publication,
                    replayed: false,
                };
                store_outbox(transaction, &bundle.event).await?;
                store_idempotency(transaction, &bundle.idempotency, &result).await?;
                Ok(result)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
) -> Result<Option<GatewayRouteCutover>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayRouteCutovers>()
                .select(CutoverSelection)
                .filter(GatewayRouteCutovers::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRouteCutovers::deployment_id().eq(deployment_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .map(CutoverRow::cutover)
        .transpose()
}

pub(super) async fn lock_by_gateway_identity(
    transaction: &PostgresTransaction,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
) -> Result<Option<GatewayRouteCutover>, PostgresPersistenceError> {
    fetch_optional::<CutoverRow, _>(
        transaction,
        select_from::<GatewayRouteCutovers>()
            .select(CutoverSelection)
            .filter(GatewayRouteCutovers::node_id().eq(node_id))
            .filter(GatewayRouteCutovers::gateway_revision().eq(gateway_revision))
            .filter(GatewayRouteCutovers::gateway_command_id().eq(gateway_command_id))
            .for_update(),
    )
    .await?
    .map(CutoverRow::cutover)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn persist_acknowledgement(
    transaction: &PostgresTransaction,
    cutover: &GatewayRouteCutover,
) -> Result<(), PostgresPersistenceError> {
    if cutover.state == GatewayRouteCutoverState::Applied {
        for candidate in &cutover.routes {
            let current = fetch_optional::<RouteRow, _>(
                transaction,
                select_from::<Routes>()
                    .select(RouteSelection)
                    .filter(Routes::id().eq(candidate.id.as_uuid()))
                    .for_update(),
            )
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant("Gateway cutover route disappeared".into())
            })?
            .route()?;
            validate_applied_route(&current, candidate, cutover)?;
            let expected_version = candidate.aggregate_version.checked_sub(2).ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway cutover route version underflowed".into(),
                )
            })?;
            let gateway_revision = candidate.gateway_revision.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway cutover route omitted its revision".into(),
                )
            })?;
            let gateway_command_id = candidate.gateway_command_id.ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway cutover route omitted its command".into(),
                )
            })?;
            let snapshot_digest = candidate.snapshot_digest.as_deref().ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway cutover route omitted its snapshot digest".into(),
                )
            })?;
            require_one_row(
                "Gateway route cutover",
                execute(
                    transaction,
                    update_table::<Routes>()
                        .set(
                            Routes::workload_revision_id(),
                            candidate.target.workload_revision_id.as_uuid(),
                        )
                        .set(
                            Routes::runtime_unit_id(),
                            candidate.target.runtime_unit_id.as_str(),
                        )
                        .set(
                            Routes::runtime_generation(),
                            candidate.target.runtime_generation,
                        )
                        .set(Routes::port_name(), candidate.target.port_name.as_str())
                        .set(
                            Routes::upstream_origin(),
                            candidate.target.upstream.as_str(),
                        )
                        .set(Routes::target_observed_at(), candidate.target.observed_at)
                        .set(Routes::state(), candidate.state.as_str())
                        .set(Routes::gateway_revision(), gateway_revision)
                        .set(Routes::gateway_command_id(), gateway_command_id.as_uuid())
                        .set(Routes::snapshot_digest(), snapshot_digest)
                        .set(Routes::failure(), candidate.failure.clone())
                        .set(Routes::aggregate_version(), candidate.aggregate_version)
                        .set(Routes::updated_at(), candidate.updated_at)
                        .set(Routes::activated_at(), candidate.activated_at)
                        .set(
                            Routes::gateway_certificate_id(),
                            candidate.gateway_certificate_id.map(|id| id.as_uuid()),
                        )
                        .filter(Routes::id().eq(candidate.id.as_uuid()))
                        .filter(Routes::aggregate_version().eq(expected_version)),
                )
                .await?,
            )?;
        }
    }
    let routes = serde_json::to_value(&cutover.routes)
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    require_one_row(
        "Gateway route cutover acknowledgement",
        execute(
            transaction,
            update_table::<GatewayRouteCutovers>()
                .set(GatewayRouteCutovers::routes(), routes)
                .set(GatewayRouteCutovers::state(), cutover.state.as_str())
                .set(GatewayRouteCutovers::failure(), cutover.failure.clone())
                .set(
                    GatewayRouteCutovers::acknowledged_at(),
                    cutover.acknowledged_at,
                )
                .filter(GatewayRouteCutovers::deployment_id().eq(cutover.deployment_id.as_uuid()))
                .filter(GatewayRouteCutovers::state().eq("pending")),
        )
        .await?,
    )
}

async fn insert_cutover(
    transaction: &PostgresTransaction,
    cutover: &GatewayRouteCutover,
) -> Result<(), PostgresPersistenceError> {
    let routes = serde_json::to_value(&cutover.routes)
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    let result = execute(
        transaction,
        insert_into::<GatewayRouteCutovers>()
            .value(
                GatewayRouteCutovers::deployment_id(),
                cutover.deployment_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::organization_id(),
                cutover.organization_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::workload_id(),
                cutover.workload_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::previous_revision_id(),
                cutover.previous_revision_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::candidate_revision_id(),
                cutover.candidate_revision_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::previous_generation(),
                cutover.previous_generation,
            )
            .value(
                GatewayRouteCutovers::candidate_generation(),
                cutover.candidate_generation,
            )
            .value(GatewayRouteCutovers::node_id(), cutover.node_id.as_uuid())
            .value(
                GatewayRouteCutovers::gateway_revision(),
                cutover.gateway_revision,
            )
            .value(
                GatewayRouteCutovers::gateway_command_id(),
                cutover.gateway_command_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::gateway_certificate_id(),
                cutover.gateway_certificate_id.as_uuid(),
            )
            .value(
                GatewayRouteCutovers::snapshot_digest(),
                cutover.snapshot_digest.as_str(),
            )
            .value(
                GatewayRouteCutovers::snapshot_expires_at(),
                cutover.snapshot_expires_at,
            )
            .value(GatewayRouteCutovers::routes(), routes)
            .value(GatewayRouteCutovers::state(), cutover.state.as_str())
            .value(GatewayRouteCutovers::failure(), cutover.failure.clone())
            .value(GatewayRouteCutovers::staged_at(), cutover.staged_at)
            .value(
                GatewayRouteCutovers::acknowledged_at(),
                cutover.acknowledged_at,
            ),
    )
    .await;
    match result {
        Ok(rows) => require_one_row("Gateway route cutover", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "Gateway route cutover identity already exists".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

fn validate_pending_routes(
    active_routes: &[Route],
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    let current = active_routes
        .iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    let candidates = cutover
        .routes
        .iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    if current.is_empty()
        || current.len() != candidates.len()
        || current.keys().ne(candidates.keys())
    {
        return Err(RepositoryError::Conflict(
            "Gateway route cutover must replace every active route for the previous revision"
                .into(),
        ));
    }
    for (route_id, candidate) in candidates {
        let route = current
            .get(&route_id)
            .copied()
            .ok_or(RepositoryError::NotFound)?;
        if !same_route_ownership(route, candidate)
            || route.state != RouteState::Active
            || route.target.workload_revision_id != cutover.previous_revision_id
            || route.target.runtime_generation != cutover.previous_generation
            || route.gateway_node_id != cutover.node_id
            || candidate.state != RouteState::Publishing
            || candidate.target.workload_revision_id != cutover.candidate_revision_id
            || candidate.target.runtime_generation != cutover.candidate_generation
            || candidate.gateway_certificate_id == route.gateway_certificate_id
            || candidate.aggregate_version != route.aggregate_version.saturating_add(1)
            || candidate.updated_at < route.updated_at
        {
            return Err(RepositoryError::Conflict(
                "active route changed while staging its Gateway cutover".into(),
            ));
        }
    }
    Ok(())
}

fn validate_applied_route(
    current: &Route,
    candidate: &Route,
    cutover: &GatewayRouteCutover,
) -> Result<(), RepositoryError> {
    if !same_route_ownership(current, candidate)
        || current.state != RouteState::Active
        || current.target.workload_revision_id != cutover.previous_revision_id
        || current.target.runtime_generation != cutover.previous_generation
        || candidate.state != RouteState::Active
        || candidate.target.workload_revision_id != cutover.candidate_revision_id
        || candidate.target.runtime_generation != cutover.candidate_generation
        || candidate.aggregate_version != current.aggregate_version.saturating_add(2)
        || candidate.updated_at < current.updated_at
    {
        return Err(RepositoryError::Conflict(
            "active route changed before applying its Gateway cutover".into(),
        ));
    }
    Ok(())
}

fn same_route_ownership(current: &Route, candidate: &Route) -> bool {
    current.id == candidate.id
        && current.organization_id == candidate.organization_id
        && current.project_id == candidate.project_id
        && current.environment_id == candidate.environment_id
        && current.gateway_scope_id == candidate.gateway_scope_id
        && current.gateway_node_id == candidate.gateway_node_id
        && current.hostname == candidate.hostname
        && current.path_prefix == candidate.path_prefix
        && current.domain_claim_id == candidate.domain_claim_id
        && current.domain_pattern == candidate.domain_pattern
        && current.workload_id == candidate.workload_id
        && current.target.port_name == candidate.target.port_name
        && current.created_at == candidate.created_at
}

fn validate_scope(
    last_issued_revision: u64,
    installed_revision: Option<u64>,
    aggregate_version: u64,
) -> Result<(), RepositoryError> {
    if last_issued_revision == 0
        || aggregate_version == 0
        || installed_revision
            .is_some_and(|installed| installed == 0 || installed > last_issued_revision)
    {
        return Err(RepositoryError::Storage(
            "stored Gateway scope state is invalid".into(),
        ));
    }
    Ok(())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored route {label} is invalid: {error}"))
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
