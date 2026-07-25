use super::postgres::{
    insert_publication, query_routes, PublicationRow, PublicationSelection, RouteRow,
    RouteSelection,
};
use super::postgres_tls::{insert_certificate, CertificateRow, CertificateSelection};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, StageGatewayCertificateConvergence,
};
use crate::modules::edge::domain::{
    DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceReason, GatewayCertificateConvergenceState,
    GatewayCertificateState, GatewayPublicationState, GatewayRouteVersion, GatewayScopeState,
    Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId,
    RepositoryError, RouteId,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    coalesce, exists, insert_into, least, min, not, orm_table, scalar_subquery, select_from,
    select_from_as, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::postgres_schema::{
    DomainClaims, GatewayCertificateConvergences, GatewayCertificates, GatewayPublications,
    GatewayScopes, Nodes, Routes,
};

orm_table! {
    struct InstalledPublications => "installed" {
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        command_id: Uuid => "command_id",
        snapshot_digest: String => "snapshot_digest",
        state: String => "state",
        snapshot_expires_at: DateTime<Utc> => "snapshot_expires_at",
    }
}

orm_table! {
    struct PendingPublications => "pending" {
        node_id: Uuid => "node_id",
        revision: u64 => "revision",
        state: String => "state",
    }
}

orm_table! {
    struct ActiveRoutes => "active_route" {
        id: Uuid => "id",
        gateway_node_id: Uuid => "gateway_node_id",
        state: String => "state",
    }
}

orm_table! {
    struct CandidateRoutes => "candidate_route" {
        id: Uuid => "id",
        gateway_node_id: Uuid => "gateway_node_id",
        state: String => "state",
        gateway_revision: u64 => "gateway_revision",
        gateway_command_id: Uuid => "gateway_command_id",
        snapshot_digest: String => "snapshot_digest",
        domain_claim_id: Option<Uuid> => "domain_claim_id",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
    }
}

orm_table! {
    struct CandidateClaims => "candidate_claim" {
        id: Uuid => "id",
        state: String => "state",
    }
}

orm_table! {
    struct CandidateCertificates => "candidate_certificate" {
        id: Uuid => "id",
        node_id: Uuid => "node_id",
        state: String => "state",
        expires_at: Option<DateTime<Utc>> => "expires_at",
    }
}

orm_table! {
    struct ExpiryRoutes => "expiry_route" {
        gateway_node_id: Uuid => "gateway_node_id",
        gateway_certificate_id: Option<Uuid> => "gateway_certificate_id",
        state: String => "state",
    }
}

orm_table! {
    struct ExpiryCertificates => "expiry_certificate" {
        id: Uuid => "id",
        expires_at: Option<DateTime<Utc>> => "expires_at",
    }
}

struct ConvergenceRow {
    organization_id: Uuid,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
    previous_certificate_id: Uuid,
    replacement_certificate_id: Option<Uuid>,
    snapshot_digest: String,
    retained_routes: serde_json::Value,
    rejected_routes: serde_json::Value,
    reason: String,
    state: String,
    failure: Option<String>,
    staged_at: DateTime<Utc>,
    acknowledged_at: Option<DateTime<Utc>>,
}

struct ConvergenceSelection;

impl Selection for ConvergenceSelection {
    type Output = ConvergenceRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayCertificateConvergences::organization_id().expression(),
            GatewayCertificateConvergences::node_id().expression(),
            GatewayCertificateConvergences::gateway_revision().expression(),
            GatewayCertificateConvergences::gateway_command_id().expression(),
            GatewayCertificateConvergences::previous_certificate_id().expression(),
            GatewayCertificateConvergences::replacement_certificate_id().expression(),
            GatewayCertificateConvergences::snapshot_digest().expression(),
            GatewayCertificateConvergences::retained_routes().expression(),
            GatewayCertificateConvergences::rejected_routes().expression(),
            GatewayCertificateConvergences::reason().expression(),
            GatewayCertificateConvergences::state().expression(),
            GatewayCertificateConvergences::failure().expression(),
            GatewayCertificateConvergences::staged_at().expression(),
            GatewayCertificateConvergences::acknowledged_at().expression(),
        ]
    }
}

impl FromRow for ConvergenceRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            node_id: decode(row, 1)?,
            gateway_revision: decode(row, 2)?,
            gateway_command_id: decode(row, 3)?,
            previous_certificate_id: decode(row, 4)?,
            replacement_certificate_id: decode(row, 5)?,
            snapshot_digest: decode(row, 6)?,
            retained_routes: decode(row, 7)?,
            rejected_routes: decode(row, 8)?,
            reason: decode(row, 9)?,
            state: decode(row, 10)?,
            failure: decode(row, 11)?,
            staged_at: decode(row, 12)?,
            acknowledged_at: decode(row, 13)?,
        })
    }
}

impl ConvergenceRow {
    fn convergence(self) -> Result<GatewayCertificateConvergence, RepositoryError> {
        let convergence = GatewayCertificateConvergence {
            organization_id: crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                self.organization_id,
            ),
            node_id: NodeId::from_uuid(self.node_id),
            gateway_revision: self.gateway_revision,
            gateway_command_id: NodeCommandId::from_uuid(self.gateway_command_id),
            previous_certificate_id: GatewayCertificateId::from_uuid(self.previous_certificate_id),
            replacement_certificate_id: self
                .replacement_certificate_id
                .map(GatewayCertificateId::from_uuid),
            snapshot_digest: self.snapshot_digest,
            retained_routes: serde_json::from_value(self.retained_routes)
                .map_err(|error| stored("retained routes")(error.to_string()))?,
            rejected_routes: serde_json::from_value(self.rejected_routes)
                .map_err(|error| stored("rejected routes")(error.to_string()))?,
            reason: crate::modules::edge::domain::GatewayCertificateConvergenceReason::parse(
                &self.reason,
            )
            .map_err(stored("reason"))?,
            state: GatewayCertificateConvergenceState::parse(&self.state)
                .map_err(stored("state"))?,
            failure: self.failure,
            staged_at: self.staged_at,
            acknowledged_at: self.acknowledged_at,
        };
        convergence.validate().map_err(stored("projection"))?;
        Ok(convergence)
    }
}

pub(super) async fn targets(
    executor: &PostgresExecutor,
    certificate_renew_before: DateTime<Utc>,
    snapshot_renew_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError> {
    validate_limit(limit)?;
    let certificate_renew_before = canonical_timestamp(certificate_renew_before);
    let snapshot_renew_before = canonical_timestamp(snapshot_renew_before);
    let active_routes = exists(
        select_from_as::<Routes, ActiveRoutes>()
            .select(ActiveRoutes::id())
            .filter(ActiveRoutes::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(ActiveRoutes::state().eq("active")),
    );
    let pending_publication = exists(
        select_from_as::<GatewayPublications, PendingPublications>()
            .select(PendingPublications::revision())
            .filter(PendingPublications::node_id().eq_column(GatewayScopes::node_id()))
            .filter(PendingPublications::state().eq("pending")),
    );
    let projection_drift = exists(
        select_from_as::<Routes, CandidateRoutes>()
            .select(CandidateRoutes::id())
            .left_join_as::<DomainClaims, CandidateClaims>(
                CandidateClaims::id().eq_column(CandidateRoutes::domain_claim_id()),
            )
            .left_join_as::<GatewayCertificates, CandidateCertificates>(
                CandidateCertificates::id().eq_column(CandidateRoutes::gateway_certificate_id()),
            )
            .filter(CandidateRoutes::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(CandidateRoutes::state().eq("active"))
            .filter(
                CandidateClaims::id()
                    .is_null()
                    .or(CandidateClaims::state().ne("verified"))
                    .or(CandidateRoutes::gateway_revision()
                        .ne_column(GatewayScopes::installed_revision()))
                    .or(CandidateRoutes::gateway_command_id()
                        .ne_column(InstalledPublications::command_id()))
                    .or(CandidateRoutes::snapshot_digest()
                        .ne_column(InstalledPublications::snapshot_digest()))
                    .or(CandidateCertificates::id().is_null())
                    .or(CandidateCertificates::node_id().ne_column(GatewayScopes::node_id()))
                    .or(CandidateCertificates::state().ne("ready"))
                    .or(CandidateCertificates::expires_at().lte(Some(certificate_renew_before))),
            ),
    );
    let minimum_certificate_expiry = scalar_subquery(
        select_from_as::<Routes, ExpiryRoutes>()
            .select(min(ExpiryCertificates::expires_at()))
            .inner_join_as::<GatewayCertificates, ExpiryCertificates>(
                ExpiryCertificates::id().eq_column(ExpiryRoutes::gateway_certificate_id()),
            )
            .filter(ExpiryRoutes::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(ExpiryRoutes::state().eq("active")),
    );
    let convergence_deadline = least::<DateTime<Utc>>([
        InstalledPublications::snapshot_expires_at().expression(),
        coalesce::<DateTime<Utc>>([
            minimum_certificate_expiry.expression(),
            InstalledPublications::snapshot_expires_at().expression(),
        ])
        .expression(),
    ]);
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict(
            "Gateway certificate convergence limit exceeds supported range".into(),
        )
    })?;
    let node_ids = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayScopes>()
                .select(GatewayScopes::node_id())
                .inner_join_as::<GatewayPublications, InstalledPublications>(
                    InstalledPublications::node_id()
                        .eq_column(GatewayScopes::node_id())
                        .and(
                            InstalledPublications::revision()
                                .eq_column(GatewayScopes::installed_revision()),
                        )
                        .and(InstalledPublications::state().eq("applied")),
                )
                .filter(GatewayScopes::installed_revision().is_not_null())
                .filter(active_routes)
                .filter(not(pending_publication))
                .filter(
                    InstalledPublications::snapshot_expires_at()
                        .lte(snapshot_renew_before)
                        .or(projection_drift),
                )
                .order_by_expression(convergence_deadline, OrderDirection::Asc)
                .order_by(GatewayScopes::node_id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut targets = Vec::with_capacity(node_ids.len());
    for node_id in node_ids {
        targets.push(load_target(executor, NodeId::from_uuid(node_id)).await?);
    }
    Ok(targets)
}

pub(super) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<GatewayCertificateConvergenceResult>, RepositoryError> {
    validate_limit(limit)?;
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict(
            "Gateway certificate convergence limit exceeds supported range".into(),
        )
    })?;
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayCertificateConvergences>()
                .select(ConvergenceSelection)
                .filter(GatewayCertificateConvergences::state().eq("pending"))
                .order_by(
                    GatewayCertificateConvergences::staged_at(),
                    OrderDirection::Asc,
                )
                .order_by(
                    GatewayCertificateConvergences::node_id(),
                    OrderDirection::Asc,
                )
                .order_by(
                    GatewayCertificateConvergences::gateway_revision(),
                    OrderDirection::Asc,
                )
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut results = Vec::with_capacity(rows.len());
    for row in rows {
        results.push(load_result(executor, row.convergence()?).await?);
    }
    Ok(results)
}

pub(super) async fn stage(
    executor: &PostgresExecutor,
    bundle: StageGatewayCertificateConvergence,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let convergence = &bundle.convergence;
                let organization_id = fetch_optional::<Uuid, _>(
                    transaction,
                    select_from::<Nodes>()
                        .select(Nodes::organization_id())
                        .filter(Nodes::id().eq(convergence.node_id.as_uuid()))
                        .for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if organization_id != convergence.organization_id.as_uuid() {
                    return Err(RepositoryError::NotFound.into());
                }
                let (last_issued_revision, installed_revision, aggregate_version) =
                    fetch_optional::<(u64, Option<u64>, u64), _>(
                        transaction,
                        select_from::<GatewayScopes>()
                            .select((
                                GatewayScopes::last_issued_revision(),
                                GatewayScopes::installed_revision(),
                                GatewayScopes::aggregate_version(),
                            ))
                            .filter(GatewayScopes::node_id().eq(convergence.node_id.as_uuid()))
                            .for_update(),
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Conflict(
                            "Gateway certificate convergence requires an installed scope".into(),
                        )
                    })?;
                let scope = GatewayScopeState {
                    node_id: convergence.node_id,
                    last_issued_revision,
                    installed_revision,
                    aggregate_version,
                };
                validate_scope(&scope)?;
                if scope.aggregate_version != bundle.expected_scope_version
                    || scope.installed_revision != bundle.publication.expected_revision
                    || bundle.publication.revision
                        != scope.next_revision().map_err(RepositoryError::Conflict)?
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway scope changed while compiling certificate convergence".into(),
                    )
                    .into());
                }
                if fetch_optional::<u64, _>(
                    transaction,
                    select_from::<GatewayPublications>()
                        .select(GatewayPublications::revision())
                        .filter(GatewayPublications::node_id().eq(convergence.node_id.as_uuid()))
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
                let previous = fetch_optional::<CertificateRow, _>(
                    transaction,
                    select_from::<GatewayCertificates>()
                        .select(CertificateSelection)
                        .filter(
                            GatewayCertificates::id()
                                .eq(convergence.previous_certificate_id.as_uuid()),
                        )
                        .for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .certificate()?;
                if previous.organization_id != convergence.organization_id
                    || previous.node_id != convergence.node_id
                    || !matches!(
                        previous.state,
                        GatewayCertificateState::Ready | GatewayCertificateState::Revoked
                    )
                {
                    return Err(RepositoryError::Conflict(
                        "Gateway convergence previous certificate is not authoritative".into(),
                    )
                    .into());
                }
                let active = fetch_all::<RouteRow, _>(
                    transaction,
                    select_from::<Routes>()
                        .select(RouteSelection)
                        .filter(Routes::gateway_node_id().eq(convergence.node_id.as_uuid()))
                        .filter(Routes::state().eq("active"))
                        .order_by(Routes::id(), OrderDirection::Asc)
                        .for_update(),
                )
                .await?
                .into_iter()
                .map(RouteRow::route)
                .collect::<Result<Vec<_>, _>>()?;
                validate_convergence_routes(transaction, convergence, &active).await?;
                validate_active_certificate(previous.id, &active)?;
                if convergence.reason == GatewayCertificateConvergenceReason::SnapshotRenewal {
                    let current_publication = fetch_optional::<PublicationRow, _>(
                        transaction,
                        select_from::<GatewayPublications>()
                            .select(PublicationSelection)
                            .filter(
                                GatewayPublications::node_id().eq(convergence.node_id.as_uuid()),
                            )
                            .filter(GatewayPublications::revision().eq(
                                scope.installed_revision.ok_or_else(|| {
                                    RepositoryError::Storage(
                                        "Gateway snapshot renewal has no installed revision".into(),
                                    )
                                })?,
                            ))
                            .for_update(),
                    )
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "Gateway snapshot renewal publication disappeared".into(),
                        )
                    })?
                    .publication()?;
                    if current_publication.state != GatewayPublicationState::Applied
                        || current_publication
                            .acknowledged_at
                            .is_none_or(|acknowledged_at| {
                                bundle.publication.command_issued_at < acknowledged_at
                            })
                        || bundle.publication.acl != current_publication.acl
                        || bundle.publication.snapshot_digest != current_publication.snapshot_digest
                        || bundle.publication.certificate_request.is_some()
                    {
                        return Err(RepositoryError::Conflict(
                            "Gateway snapshot renewal changed the installed policy".into(),
                        )
                        .into());
                    }
                }
                if let Some(certificate) = &bundle.certificate {
                    validate_replacement_claims(convergence, certificate, &active)?;
                }

                insert_publication(transaction, &bundle.publication).await?;
                if let Some(certificate) = &bundle.certificate {
                    insert_certificate(transaction, certificate).await?;
                }
                insert_convergence(transaction, convergence).await?;
                let next_scope_version =
                    scope.aggregate_version.checked_add(1).ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "Gateway convergence scope aggregate version overflowed".into(),
                        )
                    })?;
                require_one_row(
                    "Gateway certificate convergence scope",
                    execute(
                        transaction,
                        update_table::<GatewayScopes>()
                            .set(
                                GatewayScopes::last_issued_revision(),
                                bundle.publication.revision,
                            )
                            .set(GatewayScopes::aggregate_version(), next_scope_version)
                            .set(
                                GatewayScopes::updated_at(),
                                bundle.publication.command_issued_at,
                            )
                            .filter(
                                GatewayScopes::node_id().eq(bundle.publication.node_id.as_uuid()),
                            )
                            .filter(GatewayScopes::aggregate_version().eq(scope.aggregate_version)),
                    )
                    .await?,
                )?;
                store_outbox(transaction, &bundle.event).await?;
                Ok(GatewayCertificateConvergenceResult {
                    convergence: bundle.convergence,
                    certificate: bundle.certificate,
                    publication: bundle.publication,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    node_id: NodeId,
    gateway_revision: u64,
) -> Result<Option<GatewayCertificateConvergence>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayCertificateConvergences>()
                .select(ConvergenceSelection)
                .filter(GatewayCertificateConvergences::node_id().eq(node_id.as_uuid()))
                .filter(GatewayCertificateConvergences::gateway_revision().eq(gateway_revision)),
        )
        .await
        .map_err(storage)?
        .map(ConvergenceRow::convergence)
        .transpose()
}

pub(super) async fn obsolete_certificates(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<GatewayCertificate>, RepositoryError> {
    validate_limit(limit)?;
    let active_route = exists(
        select_from::<Routes>()
            .select(Routes::id())
            .filter(Routes::state().eq("active"))
            .filter(Routes::gateway_certificate_id().eq_column(GatewayCertificates::id())),
    );
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict(
            "Gateway certificate revocation limit exceeds supported range".into(),
        )
    })?;
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayCertificates>()
                .select((GatewayCertificates::node_id(), GatewayCertificates::id()))
                .inner_join::<GatewayScopes>(
                    GatewayScopes::node_id().eq_column(GatewayCertificates::node_id()),
                )
                .filter(GatewayCertificates::state().eq("ready"))
                .filter(
                    GatewayCertificates::gateway_revision()
                        .lt_column(GatewayScopes::installed_revision()),
                )
                .filter(not(active_route))
                .order_by(GatewayCertificates::gateway_revision(), OrderDirection::Asc)
                .order_by(GatewayCertificates::node_id(), OrderDirection::Asc)
                .order_by(GatewayCertificates::id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut certificates = Vec::with_capacity(identities.len());
    for (node_id, certificate_id) in identities {
        certificates.push(
            super::postgres_tls::find_gateway_certificate(
                executor,
                NodeId::from_uuid(node_id),
                GatewayCertificateId::from_uuid(certificate_id),
            )
            .await?,
        );
    }
    Ok(certificates)
}

pub(super) async fn lock_by_gateway_identity(
    transaction: &PostgresTransaction,
    node_id: Uuid,
    gateway_revision: u64,
    gateway_command_id: Uuid,
) -> Result<Option<GatewayCertificateConvergence>, PostgresPersistenceError> {
    fetch_optional::<ConvergenceRow, _>(
        transaction,
        select_from::<GatewayCertificateConvergences>()
            .select(ConvergenceSelection)
            .filter(GatewayCertificateConvergences::node_id().eq(node_id))
            .filter(GatewayCertificateConvergences::gateway_revision().eq(gateway_revision))
            .filter(GatewayCertificateConvergences::gateway_command_id().eq(gateway_command_id))
            .for_update(),
    )
    .await?
    .map(ConvergenceRow::convergence)
    .transpose()
    .map_err(Into::into)
}

pub(super) async fn persist_acknowledgement(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
) -> Result<(), PostgresPersistenceError> {
    if convergence.state == GatewayCertificateConvergenceState::Applied {
        persist_route_convergence(transaction, convergence).await?;
    }
    require_one_row(
        "Gateway certificate convergence acknowledgement",
        execute(
            transaction,
            update_table::<GatewayCertificateConvergences>()
                .set(
                    GatewayCertificateConvergences::state(),
                    convergence.state.as_str(),
                )
                .set(
                    GatewayCertificateConvergences::failure(),
                    convergence.failure.clone(),
                )
                .set(
                    GatewayCertificateConvergences::acknowledged_at(),
                    convergence.acknowledged_at,
                )
                .filter(GatewayCertificateConvergences::node_id().eq(convergence.node_id.as_uuid()))
                .filter(
                    GatewayCertificateConvergences::gateway_revision()
                        .eq(convergence.gateway_revision),
                )
                .filter(GatewayCertificateConvergences::state().eq("pending")),
        )
        .await?,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn bind_active_routes_to_certificate(
    transaction: &PostgresTransaction,
    node_id: NodeId,
    revision: u64,
    command_id: NodeCommandId,
    snapshot_digest: &str,
    certificate_id: GatewayCertificateId,
    acknowledged_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let routes = fetch_all::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .order_by(Routes::id(), OrderDirection::Asc)
            .for_update(),
    )
    .await?
    .into_iter()
    .map(RouteRow::route)
    .collect::<Result<Vec<_>, _>>()?;
    for mut route in routes {
        let expected_version = route.aggregate_version;
        if route
            .bind_gateway_certificate(
                revision,
                command_id,
                snapshot_digest.into(),
                certificate_id,
                acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?
        {
            update_route(transaction, &route, expected_version).await?;
        }
    }
    Ok(())
}

async fn load_target(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<GatewayCertificateConvergenceTarget, RepositoryError> {
    let (last_issued_revision, installed_revision, aggregate_version) =
        Database::new(PostgresDialect, executor.clone())
            .fetch_optional_as(
                select_from::<GatewayScopes>()
                    .select((
                        GatewayScopes::last_issued_revision(),
                        GatewayScopes::installed_revision(),
                        GatewayScopes::aggregate_version(),
                    ))
                    .filter(GatewayScopes::node_id().eq(node_id.as_uuid())),
            )
            .await
            .map_err(storage)?
            .ok_or_else(|| {
                RepositoryError::Storage("Gateway convergence scope disappeared".into())
            })?;
    let scope = GatewayScopeState {
        node_id,
        last_issued_revision,
        installed_revision,
        aggregate_version,
    };
    validate_scope(&scope)?;
    let installed_revision = installed_revision.ok_or_else(|| {
        RepositoryError::Storage("Gateway convergence scope has no installed revision".into())
    })?;
    let publication = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayPublications>()
                .select(PublicationSelection)
                .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(installed_revision)),
        )
        .await
        .map_err(storage)?
        .ok_or_else(|| {
            RepositoryError::Storage("installed Gateway publication disappeared".into())
        })?
        .publication()?;
    let routes = query_routes(
        executor,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .order_by(Routes::id(), OrderDirection::Asc),
    )
    .await?;
    let certificate_ids = routes
        .iter()
        .map(|route| {
            route.gateway_certificate_id.ok_or_else(|| {
                RepositoryError::Storage("active Gateway route omitted its certificate".into())
            })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if certificate_ids.len() != 1 {
        return Err(RepositoryError::Storage(
            "active Gateway routes disagree on their certificate".into(),
        ));
    }
    let certificate_id = *certificate_ids.iter().next().ok_or_else(|| {
        RepositoryError::Storage("installed Gateway scope has no active certificate".into())
    })?;
    let certificate =
        super::postgres_tls::find_gateway_certificate(executor, node_id, certificate_id).await?;
    let mut statuses = Vec::with_capacity(routes.len());
    for route in routes {
        let claim_id = route.domain_claim_id.ok_or_else(|| {
            RepositoryError::Storage("active TLS route omitted its domain claim".into())
        })?;
        statuses.push(GatewayCertificateRouteStatus {
            route,
            domain_claim_state: load_claim_state(executor, claim_id).await?,
        });
    }
    let target = GatewayCertificateConvergenceTarget {
        scope,
        publication,
        certificate,
        routes: statuses,
    };
    target.validate().map_err(RepositoryError::Storage)?;
    Ok(target)
}

async fn load_claim_state(
    executor: &PostgresExecutor,
    claim_id: DomainClaimId,
) -> Result<DomainClaimState, RepositoryError> {
    let state = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<DomainClaims>()
                .select(DomainClaims::state())
                .filter(DomainClaims::id().eq(claim_id.as_uuid())),
        )
        .await
        .map_err(storage)?
        .ok_or_else(|| RepositoryError::Storage("active route domain claim disappeared".into()))?;
    DomainClaimState::parse(&state).map_err(stored("domain claim state"))
}

async fn load_result(
    executor: &PostgresExecutor,
    convergence: GatewayCertificateConvergence,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    let publication = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayPublications>()
                .select(PublicationSelection)
                .filter(GatewayPublications::node_id().eq(convergence.node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(convergence.gateway_revision)),
        )
        .await
        .map_err(storage)?
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway certificate convergence publication disappeared".into(),
            )
        })?
        .publication()?;
    let certificate = convergence
        .replacement_certificate_id
        .map(|certificate_id| {
            super::postgres_tls::find_gateway_certificate(
                executor,
                convergence.node_id,
                certificate_id,
            )
        });
    let certificate = match certificate {
        Some(future) => Some(future.await?),
        None => None,
    };
    Ok(GatewayCertificateConvergenceResult {
        convergence,
        certificate,
        publication,
    })
}

async fn validate_convergence_routes(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
    active: &[Route],
) -> Result<(), PostgresPersistenceError> {
    let active_by_id = active
        .iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    let planned = convergence
        .retained_routes
        .iter()
        .chain(&convergence.rejected_routes)
        .map(|version| version.route_id)
        .collect::<BTreeSet<_>>();
    if active_by_id.keys().copied().collect::<BTreeSet<_>>() != planned {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence must classify every active route".into(),
        )
        .into());
    }
    validate_versions_and_claims(
        transaction,
        &active_by_id,
        &convergence.retained_routes,
        true,
    )
    .await?;
    validate_versions_and_claims(
        transaction,
        &active_by_id,
        &convergence.rejected_routes,
        false,
    )
    .await
}

fn validate_active_certificate(
    certificate_id: GatewayCertificateId,
    active: &[Route],
) -> Result<(), PostgresPersistenceError> {
    if active
        .iter()
        .any(|route| route.gateway_certificate_id != Some(certificate_id))
    {
        return Err(RepositoryError::Conflict(
            "active Gateway routes changed certificate during convergence".into(),
        )
        .into());
    }
    Ok(())
}

async fn validate_versions_and_claims(
    transaction: &PostgresTransaction,
    active: &BTreeMap<RouteId, &Route>,
    versions: &[GatewayRouteVersion],
    must_be_verified: bool,
) -> Result<(), PostgresPersistenceError> {
    for version in versions {
        let route = active
            .get(&version.route_id)
            .ok_or(RepositoryError::NotFound)?;
        let claim_id = route.domain_claim_id.ok_or_else(|| {
            RepositoryError::Storage("active TLS route omitted its domain claim".into())
        })?;
        let claim_state = fetch_optional::<String, _>(
            transaction,
            select_from::<DomainClaims>()
                .select(DomainClaims::state())
                .filter(DomainClaims::id().eq(claim_id.as_uuid()))
                .for_update(),
        )
        .await?
        .ok_or_else(|| RepositoryError::Storage("active route domain claim disappeared".into()))?;
        let claim_state = DomainClaimState::parse(&claim_state)
            .map_err(|error| stored("domain claim state")(error))?;
        if route.aggregate_version != version.aggregate_version
            || (claim_state == DomainClaimState::Verified) != must_be_verified
        {
            return Err(RepositoryError::Conflict(
                "active route or domain ownership changed during certificate convergence".into(),
            )
            .into());
        }
    }
    Ok(())
}

fn validate_replacement_claims(
    convergence: &GatewayCertificateConvergence,
    certificate: &GatewayCertificate,
    active: &[Route],
) -> Result<(), PostgresPersistenceError> {
    let active_by_id = active
        .iter()
        .map(|route| (route.id, route))
        .collect::<BTreeMap<_, _>>();
    let mut expected_claims = convergence
        .retained_routes
        .iter()
        .filter_map(|version| {
            active_by_id
                .get(&version.route_id)
                .and_then(|route| route.domain_claim_id)
        })
        .collect::<Vec<_>>();
    expected_claims.sort();
    expected_claims.dedup();
    if certificate.domain_claim_ids != expected_claims {
        return Err(RepositoryError::Conflict(
            "Gateway replacement certificate does not cover retained route claims".into(),
        )
        .into());
    }
    Ok(())
}

async fn insert_convergence(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
) -> Result<(), PostgresPersistenceError> {
    let retained_routes = serde_json::to_value(&convergence.retained_routes)
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    let rejected_routes = serde_json::to_value(&convergence.rejected_routes)
        .map_err(|error| PostgresPersistenceError::Invariant(error.to_string()))?;
    require_one_row(
        "Gateway certificate convergence",
        execute(
            transaction,
            insert_into::<GatewayCertificateConvergences>()
                .value(
                    GatewayCertificateConvergences::organization_id(),
                    convergence.organization_id.as_uuid(),
                )
                .value(
                    GatewayCertificateConvergences::node_id(),
                    convergence.node_id.as_uuid(),
                )
                .value(
                    GatewayCertificateConvergences::gateway_revision(),
                    convergence.gateway_revision,
                )
                .value(
                    GatewayCertificateConvergences::gateway_command_id(),
                    convergence.gateway_command_id.as_uuid(),
                )
                .value(
                    GatewayCertificateConvergences::previous_certificate_id(),
                    convergence.previous_certificate_id.as_uuid(),
                )
                .value(
                    GatewayCertificateConvergences::replacement_certificate_id(),
                    convergence
                        .replacement_certificate_id
                        .map(|certificate_id| certificate_id.as_uuid()),
                )
                .value(
                    GatewayCertificateConvergences::snapshot_digest(),
                    convergence.snapshot_digest.as_str(),
                )
                .value(
                    GatewayCertificateConvergences::retained_routes(),
                    retained_routes,
                )
                .value(
                    GatewayCertificateConvergences::rejected_routes(),
                    rejected_routes,
                )
                .value(
                    GatewayCertificateConvergences::reason(),
                    convergence.reason.as_str(),
                )
                .value(
                    GatewayCertificateConvergences::state(),
                    convergence.state.as_str(),
                )
                .value(
                    GatewayCertificateConvergences::failure(),
                    convergence.failure.clone(),
                )
                .value(
                    GatewayCertificateConvergences::staged_at(),
                    convergence.staged_at,
                )
                .value(
                    GatewayCertificateConvergences::acknowledged_at(),
                    convergence.acknowledged_at,
                ),
        )
        .await?,
    )
}

async fn persist_route_convergence(
    transaction: &PostgresTransaction,
    convergence: &GatewayCertificateConvergence,
) -> Result<(), PostgresPersistenceError> {
    let acknowledged_at = convergence.acknowledged_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "applied Gateway certificate convergence omitted acknowledgement time".into(),
        )
    })?;
    let active_certificate_id = convergence.active_certificate_id();
    for version in &convergence.retained_routes {
        let mut route = lock_active_route(transaction, version).await?;
        let expected_version = route.aggregate_version;
        route
            .bind_gateway_certificate(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                active_certificate_id.ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "retained convergence route has no active certificate".into(),
                    )
                })?,
                acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
        update_route(transaction, &route, expected_version).await?;
    }
    for version in &convergence.rejected_routes {
        let mut route = lock_active_route(transaction, version).await?;
        let expected_version = route.aggregate_version;
        route
            .reject_for_domain_revocation(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
        update_route(transaction, &route, expected_version).await?;
    }
    Ok(())
}

async fn lock_active_route(
    transaction: &PostgresTransaction,
    version: &GatewayRouteVersion,
) -> Result<Route, PostgresPersistenceError> {
    let route = fetch_optional::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::id().eq(version.route_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .route()?;
    if route.state != RouteState::Active || route.aggregate_version != version.aggregate_version {
        return Err(RepositoryError::Conflict(
            "active route changed before certificate convergence acknowledgement".into(),
        )
        .into());
    }
    Ok(route)
}

async fn update_route(
    transaction: &PostgresTransaction,
    route: &Route,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let gateway_revision = route.gateway_revision.ok_or_else(|| {
        PostgresPersistenceError::Invariant("Gateway certificate route omitted its revision".into())
    })?;
    let gateway_command_id = route.gateway_command_id.ok_or_else(|| {
        PostgresPersistenceError::Invariant("Gateway certificate route omitted its command".into())
    })?;
    let snapshot_digest = route.snapshot_digest.as_deref().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Gateway certificate route omitted its snapshot digest".into(),
        )
    })?;
    require_one_row(
        "Gateway certificate route convergence",
        execute(
            transaction,
            update_table::<Routes>()
                .set(Routes::state(), route.state.as_str())
                .set(Routes::gateway_revision(), gateway_revision)
                .set(Routes::gateway_command_id(), gateway_command_id.as_uuid())
                .set(Routes::snapshot_digest(), snapshot_digest)
                .set(
                    Routes::gateway_certificate_id(),
                    route
                        .gateway_certificate_id
                        .map(|certificate_id| certificate_id.as_uuid()),
                )
                .set(Routes::failure(), route.failure.clone())
                .set(Routes::aggregate_version(), route.aggregate_version)
                .set(Routes::updated_at(), route.updated_at)
                .set(Routes::activated_at(), route.activated_at)
                .filter(Routes::id().eq(route.id.as_uuid()))
                .filter(Routes::aggregate_version().eq(expected_version)),
        )
        .await?,
    )
}

fn validate_scope(scope: &GatewayScopeState) -> Result<(), RepositoryError> {
    if scope.last_issued_revision == 0
        || scope.aggregate_version == 0
        || scope.installed_revision.is_none()
        || scope
            .installed_revision
            .is_some_and(|installed| installed == 0 || installed > scope.last_issued_revision)
    {
        return Err(RepositoryError::Storage(
            "stored Gateway scope state is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence batch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| {
        RepositoryError::Storage(format!(
            "stored Gateway certificate convergence {label} is invalid: {error}"
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
