use crate::infrastructure::{
    execute, fetch_all, fetch_optional, idempotency_replay, is_foreign_key_violation,
    is_unique_violation, require_one_row, store_idempotency, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    EdgeRoutePublicationResult, IEdgeRepository, StageRoutePublication,
};
use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayScopeState, Route, RouteHostname,
    RoutePath, RoutePortName, RouteState, UpstreamEndpoint,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError, RouteId,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_ROUTES: &str = "select id, organization_id, project_id, environment_id, gateway_node_id, hostname, path_prefix, workload_id, workload_revision_id, port_name, upstream_origin, state, gateway_revision, gateway_command_id, snapshot_digest, failure, aggregate_version, created_at, updated_at, activated_at from routes";
const SELECT_PUBLICATIONS: &str = "select node_id, revision, expected_revision, command_id, command_correlation_id, snapshot_digest, acl, state, failure, command_issued_at, command_not_after, acknowledged_at from gateway_publications";

#[derive(Clone)]
pub struct PostgresEdgeRepository {
    executor: PostgresExecutor,
}

impl PostgresEdgeRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

struct RouteRow {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    gateway_node_id: Uuid,
    hostname: String,
    path_prefix: String,
    workload_id: Uuid,
    workload_revision_id: Uuid,
    port_name: String,
    upstream_origin: String,
    state: String,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    snapshot_digest: String,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    activated_at: Option<DateTime<Utc>>,
}

impl FromRow for RouteRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            organization_id: decode(row, 1)?,
            project_id: decode(row, 2)?,
            environment_id: decode(row, 3)?,
            gateway_node_id: decode(row, 4)?,
            hostname: decode(row, 5)?,
            path_prefix: decode(row, 6)?,
            workload_id: decode(row, 7)?,
            workload_revision_id: decode(row, 8)?,
            port_name: decode(row, 9)?,
            upstream_origin: decode(row, 10)?,
            state: decode(row, 11)?,
            gateway_revision: decode(row, 12)?,
            gateway_command_id: decode(row, 13)?,
            snapshot_digest: decode(row, 14)?,
            failure: decode(row, 15)?,
            aggregate_version: decode(row, 16)?,
            created_at: decode(row, 17)?,
            updated_at: decode(row, 18)?,
            activated_at: decode(row, 19)?,
        })
    }
}

impl RouteRow {
    fn route(self) -> Result<Route, RepositoryError> {
        let route = Route {
            id: RouteId::from_uuid(self.id),
            organization_id: OrganizationId::from_uuid(self.organization_id),
            project_id: ProjectId::from_uuid(self.project_id),
            environment_id: EnvironmentId::from_uuid(self.environment_id),
            gateway_node_id: NodeId::from_uuid(self.gateway_node_id),
            hostname: RouteHostname::parse(self.hostname).map_err(stored("hostname"))?,
            path_prefix: RoutePath::parse(self.path_prefix).map_err(stored("path"))?,
            workload_id: WorkloadId::from_uuid(self.workload_id),
            workload_revision_id: WorkloadRevisionId::from_uuid(self.workload_revision_id),
            port_name: RoutePortName::parse(self.port_name).map_err(stored("port name"))?,
            upstream: UpstreamEndpoint::parse(self.upstream_origin)
                .map_err(stored("upstream endpoint"))?,
            state: RouteState::parse(&self.state).map_err(stored("state"))?,
            gateway_revision: Some(self.gateway_revision),
            gateway_command_id: Some(NodeCommandId::from_uuid(self.gateway_command_id)),
            snapshot_digest: Some(self.snapshot_digest),
            failure: self.failure,
            aggregate_version: self.aggregate_version,
            created_at: self.created_at,
            updated_at: self.updated_at,
            activated_at: self.activated_at,
        };
        validate_stored_route(&route)?;
        Ok(route)
    }
}

struct PublicationRow {
    node_id: Uuid,
    revision: u64,
    expected_revision: Option<u64>,
    command_id: Uuid,
    command_correlation_id: Uuid,
    snapshot_digest: String,
    acl: String,
    state: String,
    failure: Option<String>,
    command_issued_at: DateTime<Utc>,
    command_not_after: DateTime<Utc>,
    acknowledged_at: Option<DateTime<Utc>>,
}

impl FromRow for PublicationRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            node_id: decode(row, 0)?,
            revision: decode(row, 1)?,
            expected_revision: decode(row, 2)?,
            command_id: decode(row, 3)?,
            command_correlation_id: decode(row, 4)?,
            snapshot_digest: decode(row, 5)?,
            acl: decode(row, 6)?,
            state: decode(row, 7)?,
            failure: decode(row, 8)?,
            command_issued_at: decode(row, 9)?,
            command_not_after: decode(row, 10)?,
            acknowledged_at: decode(row, 11)?,
        })
    }
}

impl PublicationRow {
    fn publication(self) -> Result<GatewayPublication, RepositoryError> {
        let publication = GatewayPublication {
            node_id: NodeId::from_uuid(self.node_id),
            revision: self.revision,
            expected_revision: self.expected_revision,
            command_id: NodeCommandId::from_uuid(self.command_id),
            command_correlation_id: self.command_correlation_id,
            snapshot_digest: self.snapshot_digest,
            acl: self.acl,
            state: GatewayPublicationState::parse(&self.state).map_err(stored("state"))?,
            failure: self.failure,
            command_issued_at: self.command_issued_at,
            command_not_after: self.command_not_after,
            acknowledged_at: self.acknowledged_at,
        };
        publication.snapshot().map_err(stored("snapshot"))?;
        Ok(publication)
    }
}

#[async_trait]
impl IEdgeRepository for PostgresEdgeRepository {
    async fn gateway_scope(&self, node_id: NodeId) -> Result<GatewayScopeState, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<(u64, Option<u64>, u64)>(
                    "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                )
                .bind(node_id.as_uuid()),
            )
            .await
            .map_err(storage)?;
        match row {
            Some((last_issued_revision, installed_revision, aggregate_version)) => {
                validate_scope(last_issued_revision, installed_revision, aggregate_version)?;
                Ok(GatewayScopeState {
                    node_id,
                    last_issued_revision,
                    installed_revision,
                    aggregate_version,
                })
            }
            None => Ok(GatewayScopeState::empty(node_id)),
        }
    }

    async fn active_routes(&self, node_id: NodeId) -> Result<Vec<Route>, RepositoryError> {
        query_routes(
            &self.executor,
            sql_query::<RouteRow>(SELECT_ROUTES)
                .append(" where gateway_node_id = ")
                .bind(node_id.as_uuid())
                .append(" and state = 'active' order by hostname, path_prefix, id"),
        )
        .await
    }

    async fn stage_route_publication(
        &self,
        bundle: StageRoutePublication,
    ) -> Result<EdgeRoutePublicationResult, RepositoryError> {
        bundle.validate().map_err(RepositoryError::Conflict)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(mut replay) = idempotency_replay::<EdgeRoutePublicationResult>(
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
                    if organization_id != bundle.route.organization_id.as_uuid() {
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
                            "Gateway scope changed while compiling the complete snapshot".into(),
                        )
                        .into());
                    }
                    let pending = fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>(
                            "select 1 from gateway_publications where node_id = ",
                        )
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
                            "Gateway publication does not advance the authoritative scope revision"
                                .into(),
                        )
                        .into());
                    }
                    insert_publication(transaction, &bundle.publication).await?;
                    insert_route(transaction, &bundle.route).await?;
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
                                sql_query::<()>(
                                    "update gateway_scopes set last_issued_revision = ",
                                )
                                .bind(bundle.publication.revision)
                                .append(", aggregate_version = aggregate_version + 1, updated_at = ")
                                .bind(bundle.publication.command_issued_at)
                                .append(" where node_id = ")
                                .bind(bundle.publication.node_id.as_uuid())
                                .append(" and aggregate_version = ")
                                .bind(current.aggregate_version),
                            )
                            .await?,
                        )?;
                    }
                    let result = EdgeRoutePublicationResult {
                        route: bundle.route,
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

    async fn find_route(
        &self,
        organization_id: OrganizationId,
        route_id: RouteId,
    ) -> Result<Route, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<RouteRow>(SELECT_ROUTES)
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and id = ")
                    .bind(route_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .ok_or(RepositoryError::NotFound)?
            .route()
    }

    async fn list_routes(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Vec<Route>, RepositoryError> {
        query_routes(
            &self.executor,
            sql_query::<RouteRow>(SELECT_ROUTES)
                .append(" where organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and project_id = ")
                .bind(project_id.as_uuid())
                .append(" and environment_id = ")
                .bind(environment_id.as_uuid())
                .append(" order by created_at, id"),
        )
        .await
    }

    async fn project_gateway_acknowledgement(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<bool, RepositoryError> {
        acknowledgement
            .validate()
            .map_err(RepositoryError::Conflict)?;
        if received_at < acknowledgement.acknowledged_at {
            return Err(RepositoryError::Conflict(
                "Gateway acknowledgement receipt predates its node timestamp".into(),
            ));
        }
        let acknowledgement = acknowledgement.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let row = fetch_optional::<PublicationRow, _>(
                        transaction,
                        sql_query::<PublicationRow>(SELECT_PUBLICATIONS)
                            .append(" where node_id = ")
                            .bind(acknowledgement.node_id)
                            .append(" and command_id = ")
                            .bind(acknowledgement.command_id)
                            .append(" for update"),
                    )
                    .await?;
                    let Some(row) = row else {
                        return Ok(false);
                    };
                    let mut publication = row.publication()?;
                    let was_pending = publication.state == GatewayPublicationState::Pending;
                    publication
                        .acknowledge(&acknowledgement)
                        .map_err(RepositoryError::Conflict)?;
                    if !was_pending {
                        return Ok(true);
                    }
                    let rows = fetch_all::<RouteRow, _>(
                        transaction,
                        sql_query::<RouteRow>(SELECT_ROUTES)
                            .append(" where gateway_node_id = ")
                            .bind(acknowledgement.node_id)
                            .append(" and gateway_revision = ")
                            .bind(acknowledgement.revision)
                            .append(" and gateway_command_id = ")
                            .bind(acknowledgement.command_id)
                            .append(" for update"),
                    )
                    .await?;
                    if rows.is_empty() {
                        return Err(PostgresPersistenceError::Invariant(
                            "Gateway publication has no staged routes".into(),
                        ));
                    }
                    let mut routes = rows
                        .into_iter()
                        .map(RouteRow::route)
                        .collect::<Result<Vec<_>, _>>()?;
                    for route in &mut routes {
                        route
                            .apply_gateway_acknowledgement(&acknowledgement)
                            .map_err(RepositoryError::Conflict)?;
                    }
                    require_one_row(
                        "Gateway publication acknowledgement",
                        execute(
                            transaction,
                            sql_query::<()>("update gateway_publications set state = ")
                                .bind(publication.state.as_str())
                                .append(", failure = ")
                                .bind(publication.failure.as_deref())
                                .append(", acknowledged_at = ")
                                .bind(publication.acknowledged_at)
                                .append(" where node_id = ")
                                .bind(publication.node_id.as_uuid())
                                .append(" and revision = ")
                                .bind(publication.revision)
                                .append(" and state = 'pending'"),
                        )
                        .await?,
                    )?;
                    for route in routes {
                        require_one_row(
                            "route Gateway acknowledgement",
                            execute(
                                transaction,
                                sql_query::<()>("update routes set state = ")
                                    .bind(route.state.as_str())
                                    .append(", failure = ")
                                    .bind(route.failure.as_deref())
                                    .append(", aggregate_version = ")
                                    .bind(route.aggregate_version)
                                    .append(", updated_at = ")
                                    .bind(route.updated_at)
                                    .append(", activated_at = ")
                                    .bind(route.activated_at)
                                    .append(" where id = ")
                                    .bind(route.id.as_uuid())
                                    .append(" and aggregate_version = ")
                                    .bind(route.aggregate_version - 1),
                            )
                            .await?,
                        )?;
                    }
                    if acknowledgement.state == GatewayAckState::Applied {
                        require_one_row(
                            "installed Gateway scope revision",
                            execute(
                                transaction,
                                sql_query::<()>(
                                    "update gateway_scopes set installed_revision = ",
                                )
                                .bind(acknowledgement.revision)
                                .append(", aggregate_version = aggregate_version + 1, updated_at = ")
                                .bind(acknowledgement.acknowledged_at)
                                .append(" where node_id = ")
                                .bind(acknowledgement.node_id)
                                .append(" and installed_revision is not distinct from ")
                                .bind(publication.expected_revision),
                            )
                            .await?,
                        )?;
                    }
                    Ok(true)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_publication(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into gateway_publications (node_id, revision, expected_revision, command_id, command_correlation_id, snapshot_digest, acl, state, failure, command_issued_at, command_not_after, acknowledged_at) values (",
        )
        .bind(publication.node_id.as_uuid())
        .append(", ")
        .bind(publication.revision)
        .append(", ")
        .bind(publication.expected_revision)
        .append(", ")
        .bind(publication.command_id.as_uuid())
        .append(", ")
        .bind(publication.command_correlation_id)
        .append(", ")
        .bind(publication.snapshot_digest.as_str())
        .append(", ")
        .bind(publication.acl.as_str())
        .append(", ")
        .bind(publication.state.as_str())
        .append(", ")
        .bind(publication.failure.as_deref())
        .append(", ")
        .bind(publication.command_issued_at)
        .append(", ")
        .bind(publication.command_not_after)
        .append(", ")
        .bind(publication.acknowledged_at)
        .append(")"),
    )
    .await;
    map_insert("Gateway publication", result)
}

async fn insert_route(
    transaction: &a3s_orm::PostgresTransaction,
    route: &Route,
) -> Result<(), PostgresPersistenceError> {
    let result = execute(
        transaction,
        sql_query::<()>(
            "insert into routes (id, organization_id, project_id, environment_id, gateway_node_id, hostname, path_prefix, workload_id, workload_revision_id, port_name, upstream_origin, state, gateway_revision, gateway_command_id, snapshot_digest, failure, aggregate_version, created_at, updated_at, activated_at) values (",
        )
        .bind(route.id.as_uuid())
        .append(", ")
        .bind(route.organization_id.as_uuid())
        .append(", ")
        .bind(route.project_id.as_uuid())
        .append(", ")
        .bind(route.environment_id.as_uuid())
        .append(", ")
        .bind(route.gateway_node_id.as_uuid())
        .append(", ")
        .bind(route.hostname.as_str())
        .append(", ")
        .bind(route.path_prefix.as_str())
        .append(", ")
        .bind(route.workload_id.as_uuid())
        .append(", ")
        .bind(route.workload_revision_id.as_uuid())
        .append(", ")
        .bind(route.port_name.as_str())
        .append(", ")
        .bind(route.upstream.as_str())
        .append(", ")
        .bind(route.state.as_str())
        .append(", ")
        .bind(route.gateway_revision)
        .append(", ")
        .bind(route.gateway_command_id.map(|id| id.as_uuid()))
        .append(", ")
        .bind(route.snapshot_digest.as_deref())
        .append(", ")
        .bind(route.failure.as_deref())
        .append(", ")
        .bind(route.aggregate_version)
        .append(", ")
        .bind(route.created_at)
        .append(", ")
        .bind(route.updated_at)
        .append(", ")
        .bind(route.activated_at)
        .append(")"),
    )
    .await;
    map_insert("route", result)
}

fn map_insert(
    resource: &str,
    result: Result<u64, PostgresPersistenceError>,
) -> Result<(), PostgresPersistenceError> {
    match result {
        Ok(rows) => require_one_row(resource, rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "hostname and path are already owned in this Gateway scope".into(),
        )
        .into()),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn query_routes(
    executor: &PostgresExecutor,
    query: a3s_orm::SqlQuery<RouteRow>,
) -> Result<Vec<Route>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(query)
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(RouteRow::route)
        .collect()
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

fn validate_stored_route(route: &Route) -> Result<(), RepositoryError> {
    let status_consistent = match route.state {
        RouteState::Pending => false,
        RouteState::Publishing => route.failure.is_none() && route.activated_at.is_none(),
        RouteState::Active => route.failure.is_none() && route.activated_at.is_some(),
        RouteState::Rejected => route.failure.is_some() && route.activated_at.is_none(),
    };
    if !status_consistent
        || route.gateway_revision.is_none()
        || route.gateway_command_id.is_none()
        || route.snapshot_digest.is_none()
        || route.updated_at < route.created_at
    {
        return Err(RepositoryError::Storage(
            "stored route state is inconsistent".into(),
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
