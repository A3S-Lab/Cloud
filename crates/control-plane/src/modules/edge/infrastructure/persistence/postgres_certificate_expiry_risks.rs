use super::postgres::{RouteRow, RouteSelection};
use super::postgres_rollout_routes;
use super::postgres_schema::{
    GatewayCertificateExpiryRisks, GatewayCertificates, GatewayRouteProjections, Routes,
};
use super::postgres_tls::{self, CertificateRow, CertificateSelection};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::events::{
    expiry_risk_subject_id, GatewayCertificateExpiryRiskChanged,
};
use crate::modules::edge::domain::repositories::GatewayCertificateExpiryRiskTarget;
use crate::modules::edge::domain::{
    expiry_risk_deadline, GatewayCertificateExpiryRisk, GatewayCertificateExpiryRiskState,
    GatewayCertificateState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, NodeId, OrganizationId, RepositoryError, RouteId,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    exists, insert_into, not, select_from, update_table, Database, DecodeError, Expression,
    FromRow, FromValue, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
    Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct RiskRow {
    organization_id: Uuid,
    route_id: Uuid,
    node_id: Uuid,
    state: String,
    active_certificate_id: Uuid,
    active_certificate_expires_at: DateTime<Utc>,
    gateway_revision: u64,
    generation: u64,
    previous_at_risk_certificate_id: Option<Uuid>,
    previous_at_risk_certificate_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct RiskSelection;

impl Selection for RiskSelection {
    type Output = RiskRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayCertificateExpiryRisks::organization_id().expression(),
            GatewayCertificateExpiryRisks::route_id().expression(),
            GatewayCertificateExpiryRisks::node_id().expression(),
            GatewayCertificateExpiryRisks::state().expression(),
            GatewayCertificateExpiryRisks::active_certificate_id().expression(),
            GatewayCertificateExpiryRisks::active_certificate_expires_at().expression(),
            GatewayCertificateExpiryRisks::gateway_revision().expression(),
            GatewayCertificateExpiryRisks::generation().expression(),
            GatewayCertificateExpiryRisks::previous_at_risk_certificate_id().expression(),
            GatewayCertificateExpiryRisks::previous_at_risk_certificate_expires_at().expression(),
            GatewayCertificateExpiryRisks::created_at().expression(),
            GatewayCertificateExpiryRisks::updated_at().expression(),
        ]
    }
}

impl FromRow for RiskRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            route_id: decode(row, 1)?,
            node_id: decode(row, 2)?,
            state: decode(row, 3)?,
            active_certificate_id: decode(row, 4)?,
            active_certificate_expires_at: decode(row, 5)?,
            gateway_revision: decode(row, 6)?,
            generation: decode(row, 7)?,
            previous_at_risk_certificate_id: decode(row, 8)?,
            previous_at_risk_certificate_expires_at: decode(row, 9)?,
            created_at: decode(row, 10)?,
            updated_at: decode(row, 11)?,
        })
    }
}

impl RiskRow {
    fn risk(self) -> Result<GatewayCertificateExpiryRisk, RepositoryError> {
        let risk = GatewayCertificateExpiryRisk {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            route_id: RouteId::from_uuid(self.route_id),
            node_id: NodeId::from_uuid(self.node_id),
            state: GatewayCertificateExpiryRiskState::parse(&self.state)
                .map_err(stored("state"))?,
            active_certificate_id: GatewayCertificateId::from_uuid(self.active_certificate_id),
            active_certificate_expires_at: self.active_certificate_expires_at,
            gateway_revision: self.gateway_revision,
            generation: self.generation,
            previous_at_risk_certificate_id: self
                .previous_at_risk_certificate_id
                .map(GatewayCertificateId::from_uuid),
            previous_at_risk_certificate_expires_at: self.previous_at_risk_certificate_expires_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        };
        risk.validate().map_err(stored("projection"))?;
        Ok(risk)
    }
}

pub(super) async fn targets(
    executor: &PostgresExecutor,
    risk_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<GatewayCertificateExpiryRiskTarget>, RepositoryError> {
    validate_limit(limit)?;
    let risk_before = canonical_timestamp(risk_before);
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict(
            "Gateway certificate expiry-risk batch limit exceeds supported range".into(),
        )
    })?;
    let stale_projected_risk = GatewayCertificateExpiryRisks::route_id()
        .is_null()
        .or(GatewayCertificateExpiryRisks::state().ne("at_risk"))
        .or(GatewayCertificateExpiryRisks::active_certificate_id()
            .ne_column(GatewayCertificates::id()));
    let mut identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayRouteProjections>()
                .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
                .inner_join::<GatewayCertificates>(
                    GatewayCertificates::id()
                        .eq_column(GatewayRouteProjections::gateway_certificate_id()),
                )
                .left_join::<GatewayCertificateExpiryRisks>(
                    GatewayCertificateExpiryRisks::route_id()
                        .eq_column(GatewayRouteProjections::route_id())
                        .and(
                            GatewayCertificateExpiryRisks::node_id()
                                .eq_column(GatewayRouteProjections::gateway_node_id()),
                        ),
                )
                .select((
                    GatewayRouteProjections::route_id(),
                    GatewayRouteProjections::gateway_node_id(),
                    GatewayCertificates::id(),
                ))
                .filter(GatewayRouteProjections::state().eq("active"))
                .filter(Routes::state().eq("active"))
                .filter(GatewayCertificates::state().eq("ready"))
                .filter(GatewayCertificates::expires_at().lte(Some(risk_before)))
                .filter(stale_projected_risk)
                .order_by(GatewayCertificates::expires_at(), OrderDirection::Asc)
                .order_by(GatewayRouteProjections::route_id(), OrderDirection::Asc)
                .order_by(
                    GatewayRouteProjections::gateway_node_id(),
                    OrderDirection::Asc,
                )
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;

    let remaining = limit.saturating_sub(identities.len() as u64);
    if remaining > 0 {
        let projected_route = exists(
            select_from::<GatewayRouteProjections>()
                .select(GatewayRouteProjections::route_id())
                .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
        );
        let stale_legacy_risk = GatewayCertificateExpiryRisks::route_id()
            .is_null()
            .or(GatewayCertificateExpiryRisks::state().ne("at_risk"))
            .or(GatewayCertificateExpiryRisks::active_certificate_id()
                .ne_column(GatewayCertificates::id()));
        identities.extend(
            Database::new(PostgresDialect, executor.clone())
                .fetch_all_as(
                    select_from::<Routes>()
                        .inner_join::<GatewayCertificates>(
                            GatewayCertificates::id().eq_column(Routes::gateway_certificate_id()),
                        )
                        .left_join::<GatewayCertificateExpiryRisks>(
                            GatewayCertificateExpiryRisks::route_id()
                                .eq_column(Routes::id())
                                .and(
                                    GatewayCertificateExpiryRisks::node_id()
                                        .eq_column(Routes::gateway_node_id()),
                                ),
                        )
                        .select((
                            Routes::id(),
                            Routes::gateway_node_id(),
                            GatewayCertificates::id(),
                        ))
                        .filter(Routes::state().eq("active"))
                        .filter(not(projected_route))
                        .filter(GatewayCertificates::state().eq("ready"))
                        .filter(GatewayCertificates::expires_at().lte(Some(risk_before)))
                        .filter(stale_legacy_risk)
                        .order_by(GatewayCertificates::expires_at(), OrderDirection::Asc)
                        .order_by(Routes::id(), OrderDirection::Asc)
                        .order_by(Routes::gateway_node_id(), OrderDirection::Asc)
                        .limit(remaining),
                )
                .await
                .map_err(storage)?
                .rows,
        );
    }

    let mut targets = Vec::with_capacity(identities.len());
    for (route_id, node_id, certificate_id) in identities {
        let route_id = RouteId::from_uuid(route_id);
        let node_id = NodeId::from_uuid(node_id);
        let route = load_active_route(executor, route_id, node_id).await?;
        let certificate = postgres_tls::find_gateway_certificate(
            executor,
            node_id,
            GatewayCertificateId::from_uuid(certificate_id),
        )
        .await?;
        let target = GatewayCertificateExpiryRiskTarget { route, certificate };
        target
            .validate(risk_before)
            .map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    targets.sort_by_key(|target| {
        (
            target
                .certificate
                .material
                .as_ref()
                .map(|material| material.expires_at),
            target.route.id,
            target.route.gateway_node_id,
        )
    });
    Ok(targets)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn mark_at_risk(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    route_id: RouteId,
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
    observed_at: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let observed_at = canonical_timestamp(observed_at);
    let risk_before = expiry_risk_deadline(observed_at).map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let certificate = lock_certificate(transaction, certificate_id, node_id).await?;
                if certificate.organization_id != organization_id
                    || certificate.state != GatewayCertificateState::Ready
                    || certificate
                        .material
                        .as_ref()
                        .is_none_or(|material| material.expires_at > risk_before)
                {
                    return Ok(false);
                }
                let Some(route) = lock_active_route(transaction, route_id, node_id).await? else {
                    return Ok(false);
                };
                if route.organization_id != organization_id
                    || route.gateway_certificate_id != Some(certificate_id)
                {
                    return Ok(false);
                }
                let previous = lock_risk(transaction, route_id, node_id).await?;
                let Some(risk) = GatewayCertificateExpiryRisk::observe(
                    previous.as_ref(),
                    &route,
                    &certificate,
                    observed_at,
                )
                .map_err(RepositoryError::Conflict)?
                else {
                    return Ok(false);
                };
                if risk.state != GatewayCertificateExpiryRiskState::AtRisk {
                    return Err(RepositoryError::Conflict(
                        "Gateway certificate expiry-risk scan cannot infer recovery".into(),
                    )
                    .into());
                }
                let event = GatewayCertificateExpiryRiskChanged::envelope(
                    previous.as_ref(),
                    &risk,
                    &route,
                    expiry_risk_subject_id(route_id, node_id),
                )
                .map_err(RepositoryError::Storage)?;
                if !persist_transition(transaction, previous.as_ref(), &risk).await? {
                    return Ok(false);
                }
                store_outbox(transaction, &event).await?;
                Ok(true)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    route_id: RouteId,
    node_id: NodeId,
) -> Result<Option<GatewayCertificateExpiryRisk>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayCertificateExpiryRisks>()
                .select(RiskSelection)
                .filter(GatewayCertificateExpiryRisks::route_id().eq(route_id.as_uuid()))
                .filter(GatewayCertificateExpiryRisks::node_id().eq(node_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .map(RiskRow::risk)
        .transpose()
}

pub(super) async fn observe_applied_certificate(
    transaction: &PostgresTransaction,
    node_id: NodeId,
    certificate_id: GatewayCertificateId,
    observed_at: DateTime<Utc>,
    correlation_id: Uuid,
) -> Result<usize, PostgresPersistenceError> {
    let observed_at = canonical_timestamp(observed_at);
    let certificate = lock_certificate(transaction, certificate_id, node_id).await?;
    let routes = lock_active_routes(transaction, node_id).await?;
    let mut count = 0;
    for route in routes
        .into_iter()
        .filter(|route| route.gateway_certificate_id == Some(certificate_id))
    {
        let previous = lock_risk(transaction, route.id, node_id).await?;
        let Some(risk) = GatewayCertificateExpiryRisk::observe(
            previous.as_ref(),
            &route,
            &certificate,
            observed_at,
        )
        .map_err(RepositoryError::Conflict)?
        else {
            continue;
        };
        let event = GatewayCertificateExpiryRiskChanged::envelope(
            previous.as_ref(),
            &risk,
            &route,
            correlation_id,
        )
        .map_err(RepositoryError::Storage)?;
        if !persist_transition(transaction, previous.as_ref(), &risk).await? {
            return Err(PostgresPersistenceError::Invariant(
                "locked Gateway certificate expiry-risk transition lost concurrency".into(),
            ));
        }
        store_outbox(transaction, &event).await?;
        count += 1;
    }
    Ok(count)
}

async fn load_active_route(
    executor: &PostgresExecutor,
    route_id: RouteId,
    node_id: NodeId,
) -> Result<Route, RepositoryError> {
    if let Some(route) = postgres_rollout_routes::active(executor, node_id)
        .await?
        .into_iter()
        .find(|route| route.id == route_id)
    {
        return Ok(route);
    }
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<Routes>()
                .select(RouteSelection)
                .filter(Routes::id().eq(route_id.as_uuid()))
                .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
                .filter(Routes::state().eq("active")),
        )
        .await
        .map_err(storage)?
        .ok_or_else(|| RepositoryError::Storage("expiry-risk Route disappeared".into()))?
        .route()
}

async fn lock_active_route(
    transaction: &PostgresTransaction,
    route_id: RouteId,
    node_id: NodeId,
) -> Result<Option<Route>, PostgresPersistenceError> {
    if let Some(route) =
        postgres_rollout_routes::route_projection(transaction, route_id, node_id).await?
    {
        return Ok((route.state == RouteState::Active).then_some(route));
    }
    fetch_optional::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::id().eq(route_id.as_uuid()))
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .for_update(),
    )
    .await?
    .map(RouteRow::route)
    .transpose()
    .map_err(Into::into)
}

async fn lock_active_routes(
    transaction: &PostgresTransaction,
    node_id: NodeId,
) -> Result<Vec<Route>, PostgresPersistenceError> {
    let projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
    );
    let mut routes = fetch_all::<RouteRow, _>(
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
    routes.extend(postgres_rollout_routes::lock_active(transaction, node_id).await?);
    routes.sort_by_key(|route| route.id);
    Ok(routes)
}

async fn lock_certificate(
    transaction: &PostgresTransaction,
    certificate_id: GatewayCertificateId,
    node_id: NodeId,
) -> Result<crate::modules::edge::domain::GatewayCertificate, PostgresPersistenceError> {
    fetch_optional::<CertificateRow, _>(
        transaction,
        select_from::<GatewayCertificates>()
            .select(CertificateSelection)
            .filter(GatewayCertificates::id().eq(certificate_id.as_uuid()))
            .filter(GatewayCertificates::node_id().eq(node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .certificate()
    .map_err(Into::into)
}

async fn lock_risk(
    transaction: &PostgresTransaction,
    route_id: RouteId,
    node_id: NodeId,
) -> Result<Option<GatewayCertificateExpiryRisk>, PostgresPersistenceError> {
    fetch_optional::<RiskRow, _>(
        transaction,
        select_from::<GatewayCertificateExpiryRisks>()
            .select(RiskSelection)
            .filter(GatewayCertificateExpiryRisks::route_id().eq(route_id.as_uuid()))
            .filter(GatewayCertificateExpiryRisks::node_id().eq(node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .map(RiskRow::risk)
    .transpose()
    .map_err(Into::into)
}

async fn persist_transition(
    transaction: &PostgresTransaction,
    previous: Option<&GatewayCertificateExpiryRisk>,
    risk: &GatewayCertificateExpiryRisk,
) -> Result<bool, PostgresPersistenceError> {
    let rows = match previous {
        None => {
            execute(
                transaction,
                insert_into::<GatewayCertificateExpiryRisks>()
                    .value(
                        GatewayCertificateExpiryRisks::organization_id(),
                        risk.organization_id.as_uuid(),
                    )
                    .value(
                        GatewayCertificateExpiryRisks::route_id(),
                        risk.route_id.as_uuid(),
                    )
                    .value(
                        GatewayCertificateExpiryRisks::node_id(),
                        risk.node_id.as_uuid(),
                    )
                    .value(GatewayCertificateExpiryRisks::state(), risk.state.as_str())
                    .value(
                        GatewayCertificateExpiryRisks::active_certificate_id(),
                        risk.active_certificate_id.as_uuid(),
                    )
                    .value(
                        GatewayCertificateExpiryRisks::active_certificate_expires_at(),
                        risk.active_certificate_expires_at,
                    )
                    .value(
                        GatewayCertificateExpiryRisks::gateway_revision(),
                        risk.gateway_revision,
                    )
                    .value(GatewayCertificateExpiryRisks::generation(), risk.generation)
                    .value(
                        GatewayCertificateExpiryRisks::previous_at_risk_certificate_id(),
                        risk.previous_at_risk_certificate_id
                            .map(GatewayCertificateId::as_uuid),
                    )
                    .value(
                        GatewayCertificateExpiryRisks::previous_at_risk_certificate_expires_at(),
                        risk.previous_at_risk_certificate_expires_at,
                    )
                    .value(GatewayCertificateExpiryRisks::created_at(), risk.created_at)
                    .value(GatewayCertificateExpiryRisks::updated_at(), risk.updated_at)
                    .on_conflict((
                        GatewayCertificateExpiryRisks::route_id(),
                        GatewayCertificateExpiryRisks::node_id(),
                    ))
                    .do_nothing(),
            )
            .await?
        }
        Some(previous) => {
            execute(
                transaction,
                update_table::<GatewayCertificateExpiryRisks>()
                    .set(GatewayCertificateExpiryRisks::state(), risk.state.as_str())
                    .set(
                        GatewayCertificateExpiryRisks::active_certificate_id(),
                        risk.active_certificate_id.as_uuid(),
                    )
                    .set(
                        GatewayCertificateExpiryRisks::active_certificate_expires_at(),
                        risk.active_certificate_expires_at,
                    )
                    .set(
                        GatewayCertificateExpiryRisks::gateway_revision(),
                        risk.gateway_revision,
                    )
                    .set(GatewayCertificateExpiryRisks::generation(), risk.generation)
                    .set(
                        GatewayCertificateExpiryRisks::previous_at_risk_certificate_id(),
                        risk.previous_at_risk_certificate_id
                            .map(GatewayCertificateId::as_uuid),
                    )
                    .set(
                        GatewayCertificateExpiryRisks::previous_at_risk_certificate_expires_at(),
                        risk.previous_at_risk_certificate_expires_at,
                    )
                    .set(GatewayCertificateExpiryRisks::updated_at(), risk.updated_at)
                    .filter(GatewayCertificateExpiryRisks::route_id().eq(risk.route_id.as_uuid()))
                    .filter(GatewayCertificateExpiryRisks::node_id().eq(risk.node_id.as_uuid()))
                    .filter(GatewayCertificateExpiryRisks::generation().eq(previous.generation)),
            )
            .await?
        }
    };
    Ok(rows == 1)
}

fn validate_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway certificate expiry-risk batch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| {
        RepositoryError::Storage(format!(
            "stored Gateway certificate expiry-risk {label} is invalid: {error}"
        ))
    }
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
