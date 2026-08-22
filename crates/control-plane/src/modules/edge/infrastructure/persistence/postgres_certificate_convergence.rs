use super::postgres::{
    insert_publication, query_routes, PublicationRow, PublicationSelection, RouteRow,
    RouteSelection,
};
use super::postgres_tls::{
    insert_certificate, update_certificate, CertificateRow, CertificateSelection,
};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, store_outbox, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::events::GatewayCertificateExpiryChanged;
use crate::modules::edge::domain::repositories::{
    GatewayCertificateConvergenceResult, GatewayCertificateConvergenceTarget,
    GatewayCertificateRouteStatus, StageGatewayCertificateConvergence,
};
use crate::modules::edge::domain::{
    DomainClaimState, GatewayCertificate, GatewayCertificateConvergence,
    GatewayCertificateConvergenceReason, GatewayCertificateConvergenceState,
    GatewayCertificateState, GatewayPublication, GatewayPublicationState, GatewayRouteVersion,
    GatewayScopeState, Route, RouteState,
};
use crate::modules::edge::infrastructure::{
    GatewayManagedSnapshotComposition, StageManagedGatewayCertificateConvergence,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId,
    OrganizationId, RepositoryError, RouteId,
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
    GatewayRouteProjections, GatewayScopes, Nodes, Routes,
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
        id: Uuid => "id",
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
            .filter(ActiveRoutes::state().eq("active"))
            .filter(not(exists(
                select_from::<GatewayRouteProjections>()
                    .select(GatewayRouteProjections::route_id())
                    .filter(GatewayRouteProjections::route_id().eq_column(ActiveRoutes::id())),
            ))),
    );
    let active_projected_routes = exists(
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active")),
    );
    let pending_publication = exists(
        select_from_as::<GatewayPublications, PendingPublications>()
            .select(PendingPublications::revision())
            .filter(PendingPublications::node_id().eq_column(GatewayScopes::node_id()))
            .filter(PendingPublications::state().eq("pending")),
    );
    let legacy_route_drift = exists(
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
            .filter(not(exists(
                select_from::<GatewayRouteProjections>()
                    .select(GatewayRouteProjections::route_id())
                    .filter(GatewayRouteProjections::route_id().eq_column(CandidateRoutes::id())),
            )))
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
    let projected_route_drift = exists(
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .left_join_as::<DomainClaims, CandidateClaims>(
                CandidateClaims::id().eq_column(GatewayRouteProjections::domain_claim_id()),
            )
            .left_join_as::<GatewayCertificates, CandidateCertificates>(
                CandidateCertificates::id()
                    .eq_column(GatewayRouteProjections::gateway_certificate_id()),
            )
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active"))
            .filter(
                CandidateClaims::id()
                    .is_null()
                    .or(CandidateClaims::state().ne("verified"))
                    .or(GatewayRouteProjections::gateway_revision()
                        .ne_column(GatewayScopes::installed_revision()))
                    .or(GatewayRouteProjections::gateway_command_id()
                        .ne_column(InstalledPublications::command_id()))
                    .or(GatewayRouteProjections::snapshot_digest()
                        .ne_column(InstalledPublications::snapshot_digest()))
                    .or(CandidateCertificates::id().is_null())
                    .or(CandidateCertificates::node_id().ne_column(GatewayScopes::node_id()))
                    .or(CandidateCertificates::state().ne("ready"))
                    .or(CandidateCertificates::expires_at().lte(Some(certificate_renew_before))),
            ),
    );
    let minimum_legacy_certificate_expiry = scalar_subquery(
        select_from_as::<Routes, ExpiryRoutes>()
            .select(min(ExpiryCertificates::expires_at()))
            .inner_join_as::<GatewayCertificates, ExpiryCertificates>(
                ExpiryCertificates::id().eq_column(ExpiryRoutes::gateway_certificate_id()),
            )
            .filter(ExpiryRoutes::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(ExpiryRoutes::state().eq("active"))
            .filter(not(exists(
                select_from::<GatewayRouteProjections>()
                    .select(GatewayRouteProjections::route_id())
                    .filter(GatewayRouteProjections::route_id().eq_column(ExpiryRoutes::id())),
            ))),
    );
    let minimum_projected_certificate_expiry = scalar_subquery(
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .inner_join_as::<GatewayCertificates, ExpiryCertificates>(
                ExpiryCertificates::id()
                    .eq_column(GatewayRouteProjections::gateway_certificate_id()),
            )
            .select(min(ExpiryCertificates::expires_at()))
            .filter(GatewayRouteProjections::gateway_node_id().eq_column(GatewayScopes::node_id()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active")),
    );
    let convergence_deadline = least::<DateTime<Utc>>([
        InstalledPublications::snapshot_expires_at().expression(),
        coalesce::<DateTime<Utc>>([
            minimum_legacy_certificate_expiry.expression(),
            InstalledPublications::snapshot_expires_at().expression(),
        ])
        .expression(),
        coalesce::<DateTime<Utc>>([
            minimum_projected_certificate_expiry.expression(),
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
                .filter(active_routes.or(active_projected_routes))
                .filter(not(pending_publication))
                .filter(
                    InstalledPublications::snapshot_expires_at()
                        .lte(snapshot_renew_before)
                        .or(legacy_route_drift)
                        .or(projected_route_drift),
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
    stage_inner(executor, bundle, None).await
}

pub(super) async fn stage_managed(
    executor: &PostgresExecutor,
    bundle: StageManagedGatewayCertificateConvergence,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    let (ordinary, composition, previous_certificate) = bundle.into_parts();
    stage_inner(
        executor,
        ordinary,
        Some((composition, previous_certificate)),
    )
    .await
}

async fn stage_inner(
    executor: &PostgresExecutor,
    bundle: StageGatewayCertificateConvergence,
    managed: Option<(GatewayManagedSnapshotComposition, GatewayCertificate)>,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    bundle.validate().map_err(RepositoryError::Conflict)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let managed_scope = match &managed {
                    Some((composition, _)) => Some(
                        super::postgres_mcp_gateway_snapshots::lock_managed_composition(
                            transaction,
                            composition,
                        )
                        .await?,
                    ),
                    None => None,
                };
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
                if managed_scope
                    .as_ref()
                    .is_some_and(|expected| expected != &scope)
                {
                    return Err(RepositoryError::Conflict(
                        "managed Gateway scope changed while compiling certificate convergence"
                            .into(),
                    )
                    .into());
                }
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
                if managed
                    .as_ref()
                    .is_some_and(|(_, expected)| expected != &previous)
                {
                    return Err(RepositoryError::Conflict(
                        "managed Gateway previous certificate changed during convergence".into(),
                    )
                    .into());
                }
                let projected_route = exists(
                    select_from::<GatewayRouteProjections>()
                        .select(GatewayRouteProjections::route_id())
                        .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
                );
                let mut active = fetch_all::<RouteRow, _>(
                    transaction,
                    select_from::<Routes>()
                        .select(RouteSelection)
                        .filter(Routes::gateway_node_id().eq(convergence.node_id.as_uuid()))
                        .filter(Routes::state().eq("active"))
                        .filter(not(projected_route))
                        .order_by(Routes::id(), OrderDirection::Asc)
                        .for_update(),
                )
                .await?
                .into_iter()
                .map(RouteRow::route)
                .collect::<Result<Vec<_>, _>>()?;
                active.extend(
                    super::postgres_rollout_routes::lock_active(transaction, convergence.node_id)
                        .await?,
                );
                active.sort_by_key(|route| route.id);
                validate_convergence_routes(transaction, convergence, &active).await?;
                validate_active_certificate(previous.id, &active)?;
                let retained_routes = retained_routes(&active, convergence)?;
                let expected_expiry_events = GatewayCertificateExpiryChanged::envelopes(
                    convergence,
                    &bundle.publication,
                    &previous,
                    &retained_routes,
                )
                .map_err(PostgresPersistenceError::Invariant)?;
                if bundle.expiry_events != expected_expiry_events {
                    return Err(RepositoryError::Conflict(
                        "Gateway certificate expiry firing facts are inconsistent".into(),
                    )
                    .into());
                }
                if convergence.reason == GatewayCertificateConvergenceReason::SnapshotRenewal
                    && managed.is_none()
                {
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
                if managed.is_none() {
                    if let Some(certificate) = &bundle.certificate {
                        validate_replacement_claims(convergence, certificate, &active)?;
                    }
                } else if convergence.reason == GatewayCertificateConvergenceReason::SnapshotRenewal
                    && bundle.publication.certificate_request.is_some()
                {
                    return Err(RepositoryError::Conflict(
                        "managed Gateway snapshot renewal requested replacement material".into(),
                    )
                    .into());
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
                if let Some((composition, _)) = &managed {
                    super::postgres_mcp_gateway_snapshots::persist_managed_composition(
                        transaction,
                        composition,
                        &bundle.publication,
                    )
                    .await?;
                }
                store_outbox(transaction, &bundle.event).await?;
                for event in &bundle.expiry_events {
                    store_expiry_event_once(transaction, event).await?;
                }
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

#[allow(clippy::too_many_arguments)]
pub(super) async fn mark_unavailable(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    gateway_revision: u64,
    gateway_command_id: NodeCommandId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<GatewayCertificateConvergenceResult, RepositoryError> {
    let failure = failure.to_owned();
    let observed_at = canonical_timestamp(observed_at);
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let mut convergence = fetch_optional::<ConvergenceRow, _>(
                    transaction,
                    select_from::<GatewayCertificateConvergences>()
                        .select(ConvergenceSelection)
                        .filter(GatewayCertificateConvergences::node_id().eq(node_id.as_uuid()))
                        .filter(
                            GatewayCertificateConvergences::gateway_revision().eq(gateway_revision),
                        )
                        .for_update(),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .convergence()?;
                if convergence.organization_id != organization_id
                    || convergence.gateway_command_id != gateway_command_id
                {
                    return Err(RepositoryError::NotFound.into());
                }
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
                        "Gateway certificate convergence publication disappeared".into(),
                    )
                })?
                .publication()?;
                if publication.command_id != gateway_command_id {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway certificate convergence command identity diverged".into(),
                    ));
                }
                let mut certificate = match convergence.replacement_certificate_id {
                    Some(certificate_id) => Some(
                        fetch_optional::<CertificateRow, _>(
                            transaction,
                            select_from::<GatewayCertificates>()
                                .select(CertificateSelection)
                                .filter(GatewayCertificates::id().eq(certificate_id.as_uuid()))
                                .for_update(),
                        )
                        .await?
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Gateway convergence replacement certificate disappeared".into(),
                            )
                        })?
                        .certificate()?,
                    ),
                    None => None,
                };
                if certificate.as_ref().is_some_and(|certificate| {
                    certificate.organization_id != organization_id
                        || certificate.node_id != node_id
                        || certificate.gateway_revision != gateway_revision
                        || certificate.gateway_command_id != gateway_command_id
                }) {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway convergence replacement certificate identity diverged".into(),
                    ));
                }
                let publication_changed = publication
                    .mark_unavailable(&failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;
                let convergence_changed = convergence
                    .mark_unavailable(&failure, observed_at)
                    .map_err(RepositoryError::Conflict)?;
                if publication_changed != convergence_changed {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway convergence terminal projections diverged".into(),
                    ));
                }
                let certificate_version = certificate
                    .as_ref()
                    .map(|certificate| certificate.aggregate_version);
                let certificate_changed = match &mut certificate {
                    Some(certificate) => certificate
                        .mark_delivery_unavailable(&failure, observed_at)
                        .map_err(RepositoryError::Conflict)?,
                    None => false,
                };
                if publication_changed {
                    require_one_row(
                        "Gateway certificate convergence unavailable publication",
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
                    persist_acknowledgement(transaction, &convergence, &publication).await?;
                }
                if certificate_changed {
                    update_certificate(
                        transaction,
                        certificate.as_ref().ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "changed Gateway convergence certificate disappeared".into(),
                            )
                        })?,
                        certificate_version.ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "changed Gateway convergence certificate omitted its version"
                                    .into(),
                            )
                        })?,
                    )
                    .await?;
                }
                Ok(GatewayCertificateConvergenceResult {
                    convergence,
                    certificate,
                    publication,
                })
            })
        })
        .await
        .map_err(transaction_error)
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
    let active_projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active"))
            .filter(
                GatewayRouteProjections::gateway_certificate_id()
                    .eq_column(GatewayCertificates::id()),
            ),
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
                .filter(not(active_route.or(active_projected_route)))
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

mod facts;
mod route_binding;
mod support;

pub(super) use facts::persist_acknowledgement;
use facts::{retained_routes, store_expiry_event_once};
pub(super) use route_binding::bind_active_routes_to_certificate;

use support::{
    decode, insert_convergence, load_result, load_target, storage, stored,
    validate_active_certificate, validate_convergence_routes, validate_limit,
    validate_replacement_claims, validate_scope,
};
