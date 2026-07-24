use super::postgres::{insert_publication, RouteRow, SELECT_ROUTES};
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
use crate::modules::shared_kernel::domain::{
    DeploymentId, GatewayCertificateId, NodeCommandId, NodeId, OrganizationId, RepositoryError,
    WorkloadId, WorkloadRevisionId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use uuid::Uuid;

const SELECT_CUTOVERS: &str = "select deployment_id, organization_id, workload_id, previous_revision_id, candidate_revision_id, node_id, gateway_revision, gateway_command_id, gateway_certificate_id, snapshot_digest, snapshot_expires_at, routes, state, failure, staged_at, acknowledged_at from gateway_route_cutovers";

struct CutoverRow {
    deployment_id: Uuid,
    organization_id: Uuid,
    workload_id: Uuid,
    previous_revision_id: Uuid,
    candidate_revision_id: Uuid,
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

impl FromRow for CutoverRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            deployment_id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            workload_id: decode(row, 2)?,
            previous_revision_id: decode(row, 3)?,
            candidate_revision_id: decode(row, 4)?,
            node_id: decode(row, 5)?,
            gateway_revision: decode(row, 6)?,
            gateway_command_id: decode(row, 7)?,
            gateway_certificate_id: decode(row, 8)?,
            snapshot_digest: decode(row, 9)?,
            snapshot_expires_at: decode(row, 10)?,
            routes: decode(row, 11)?,
            state: decode(row, 12)?,
            failure: decode(row, 13)?,
            staged_at: decode(row, 14)?,
            acknowledged_at: decode(row, 15)?,
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
    bundle.validate().map_err(RepositoryError::Conflict)?;
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
                let organization_id = fetch_optional::<Uuid, _>(
                    transaction,
                    sql_query::<Uuid>("select organization_id from nodes where id = ")
                        .bind(bundle.publication.node_id.as_uuid())
                        .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if organization_id != bundle.cutover.organization_id.as_uuid() {
                    return Err(RepositoryError::NotFound.into());
                }
                let scope = fetch_optional::<(u64, Option<u64>, u64), _>(
                    transaction,
                    sql_query::<(u64, Option<u64>, u64)>(
                        "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                    )
                    .bind(bundle.publication.node_id.as_uuid())
                    .append(" for update"),
                )
                .await?;
                let current = match scope {
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
                };
                if current.aggregate_version != bundle.expected_scope_version {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed while compiling the route cutover snapshot".into(),
                    )
                    .into());
                }
                let pending = fetch_optional::<i32, _>(
                    transaction,
                    sql_query::<i32>("select 1 from gateway_publications where node_id = ")
                        .bind(bundle.publication.node_id.as_uuid())
                        .append(" and state = 'pending' for update"),
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
                    sql_query::<RouteRow>(SELECT_ROUTES)
                        .append(" where organization_id = ")
                        .bind(bundle.cutover.organization_id.as_uuid())
                        .append(" and workload_id = ")
                        .bind(bundle.cutover.workload_id.as_uuid())
                        .append(" and workload_revision_id = ")
                        .bind(bundle.cutover.previous_revision_id.as_uuid())
                        .append(" and state = 'active' order by id for update"),
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
                if current.aggregate_version == 0 {
                    require_one_row(
                        "Gateway scope",
                        execute(
                            transaction,
                            sql_query::<()>(
                                "insert into gateway_scopes (node_id, last_issued_revision, installed_revision, aggregate_version, updated_at) values (",
                            )
                            .bind(bundle.publication.node_id.as_uuid())
                            .append(", ")
                            .bind(bundle.publication.revision)
                            .append(", ")
                            .bind(current.installed_revision)
                            .append(", 1, ")
                            .bind(bundle.publication.command_issued_at)
                            .append(")"),
                        )
                        .await?,
                    )?;
                } else {
                    require_one_row(
                        "Gateway scope",
                        execute(
                            transaction,
                            sql_query::<()>("update gateway_scopes set last_issued_revision = ")
                                .bind(bundle.publication.revision)
                                .append(
                                    ", aggregate_version = aggregate_version + 1, updated_at = ",
                                )
                                .bind(bundle.publication.command_issued_at)
                                .append(" where node_id = ")
                                .bind(bundle.publication.node_id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(current.aggregate_version),
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
            sql_query::<CutoverRow>(SELECT_CUTOVERS)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and deployment_id = ")
                .bind(deployment_id.as_uuid()),
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
        sql_query::<CutoverRow>(SELECT_CUTOVERS)
            .append(" where node_id = ")
            .bind(node_id)
            .append(" and gateway_revision = ")
            .bind(gateway_revision)
            .append(" and gateway_command_id = ")
            .bind(gateway_command_id)
            .append(" for update"),
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
                sql_query::<RouteRow>(SELECT_ROUTES)
                    .append(" where id = ")
                    .bind(candidate.id.as_uuid())
                    .append(" for update"),
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
            require_one_row(
                "Gateway route cutover",
                execute(
                    transaction,
                    sql_query::<()>("update routes set workload_revision_id = ")
                        .bind(candidate.workload_revision_id.as_uuid())
                        .append(", upstream_origin = ")
                        .bind(candidate.upstream.as_str())
                        .append(", state = ")
                        .bind(candidate.state.as_str())
                        .append(", gateway_revision = ")
                        .bind(candidate.gateway_revision)
                        .append(", gateway_command_id = ")
                        .bind(candidate.gateway_command_id.map(|id| id.as_uuid()))
                        .append(", snapshot_digest = ")
                        .bind(candidate.snapshot_digest.as_deref())
                        .append(", failure = ")
                        .bind(candidate.failure.as_deref())
                        .append(", aggregate_version = ")
                        .bind(candidate.aggregate_version)
                        .append(", updated_at = ")
                        .bind(candidate.updated_at)
                        .append(", activated_at = ")
                        .bind(candidate.activated_at)
                        .append(", gateway_certificate_id = ")
                        .bind(candidate.gateway_certificate_id.map(|id| id.as_uuid()))
                        .append(" where id = ")
                        .bind(candidate.id.as_uuid())
                        .append(" and aggregate_version = ")
                        .bind(expected_version),
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
            sql_query::<()>("update gateway_route_cutovers set routes = ")
                .bind(routes)
                .append(", state = ")
                .bind(cutover.state.as_str())
                .append(", failure = ")
                .bind(cutover.failure.as_deref())
                .append(", acknowledged_at = ")
                .bind(cutover.acknowledged_at)
                .append(" where deployment_id = ")
                .bind(cutover.deployment_id.as_uuid())
                .append(" and state = 'pending'"),
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
        sql_query::<()>(
            "insert into gateway_route_cutovers (deployment_id, organization_id, workload_id, previous_revision_id, candidate_revision_id, node_id, gateway_revision, gateway_command_id, gateway_certificate_id, snapshot_digest, snapshot_expires_at, routes, state, failure, staged_at, acknowledged_at) values (",
        )
        .bind(cutover.deployment_id.as_uuid())
        .append(", ")
        .bind(cutover.organization_id.as_uuid())
        .append(", ")
        .bind(cutover.workload_id.as_uuid())
        .append(", ")
        .bind(cutover.previous_revision_id.as_uuid())
        .append(", ")
        .bind(cutover.candidate_revision_id.as_uuid())
        .append(", ")
        .bind(cutover.node_id.as_uuid())
        .append(", ")
        .bind(cutover.gateway_revision)
        .append(", ")
        .bind(cutover.gateway_command_id.as_uuid())
        .append(", ")
        .bind(cutover.gateway_certificate_id.as_uuid())
        .append(", ")
        .bind(cutover.snapshot_digest.as_str())
        .append(", ")
        .bind(cutover.snapshot_expires_at)
        .append(", ")
        .bind(routes)
        .append(", ")
        .bind(cutover.state.as_str())
        .append(", ")
        .bind(cutover.failure.as_deref())
        .append(", ")
        .bind(cutover.staged_at)
        .append(", ")
        .bind(cutover.acknowledged_at)
        .append(")"),
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
            || route.workload_revision_id != cutover.previous_revision_id
            || route.gateway_node_id != cutover.node_id
            || candidate.state != RouteState::Publishing
            || candidate.workload_revision_id != cutover.candidate_revision_id
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
        || current.workload_revision_id != cutover.previous_revision_id
        || candidate.state != RouteState::Active
        || candidate.workload_revision_id != cutover.candidate_revision_id
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
        && current.gateway_node_id == candidate.gateway_node_id
        && current.hostname == candidate.hostname
        && current.path_prefix == candidate.path_prefix
        && current.domain_claim_id == candidate.domain_claim_id
        && current.domain_pattern == candidate.domain_pattern
        && current.workload_id == candidate.workload_id
        && current.port_name == candidate.port_name
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
