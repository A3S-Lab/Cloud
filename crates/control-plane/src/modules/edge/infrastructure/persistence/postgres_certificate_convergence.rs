use super::postgres::{
    insert_publication, query_routes, PublicationRow, RouteRow, SELECT_PUBLICATIONS, SELECT_ROUTES,
};
use super::postgres_tls::{insert_certificate, CertificateRow, SELECT_CERTIFICATES};
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
    GatewayCertificateConvergenceState, GatewayCertificateState, GatewayRouteVersion,
    GatewayScopeState, Route, RouteState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DomainClaimId, GatewayCertificateId, NodeCommandId, NodeId,
    RepositoryError, RouteId,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

const SELECT_CONVERGENCES: &str = "select organization_id, node_id, gateway_revision, gateway_command_id, previous_certificate_id, replacement_certificate_id, snapshot_digest, retained_routes, rejected_routes, reason, state, failure, staged_at, acknowledged_at from gateway_certificate_convergences";

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
    renew_before: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<GatewayCertificateConvergenceTarget>, RepositoryError> {
    validate_limit(limit)?;
    let node_ids = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<Uuid>(
                "select scope.node_id from gateway_scopes scope join gateway_certificates certificate on certificate.node_id = scope.node_id and certificate.gateway_revision = scope.installed_revision where scope.installed_revision is not null and certificate.state in ('ready', 'revoked') and exists (select 1 from routes active_route where active_route.gateway_node_id = scope.node_id and active_route.state = 'active') and not exists (select 1 from gateway_publications pending where pending.node_id = scope.node_id and pending.state = 'pending') and (certificate.state = 'revoked' or certificate.expires_at <= ",
            )
            .bind(canonical_timestamp(renew_before))
            .append(
                " or exists (select 1 from routes route left join domain_claims claim on claim.id = route.domain_claim_id where route.gateway_node_id = scope.node_id and route.state = 'active' and (claim.state is distinct from 'verified' or route.gateway_revision is distinct from scope.installed_revision or route.gateway_command_id is distinct from certificate.gateway_command_id or route.snapshot_digest is distinct from certificate.snapshot_digest or route.gateway_certificate_id is distinct from certificate.id))) order by certificate.expires_at, scope.node_id limit ",
            )
            .bind(u64::try_from(limit).map_err(|_| {
                RepositoryError::Conflict(
                    "Gateway certificate convergence limit exceeds supported range".into(),
                )
            })?),
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
    let rows = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<ConvergenceRow>(SELECT_CONVERGENCES)
                .append(
                    " where state = 'pending' order by staged_at, node_id, gateway_revision limit ",
                )
                .bind(u64::try_from(limit).map_err(|_| {
                    RepositoryError::Conflict(
                        "Gateway certificate convergence limit exceeds supported range".into(),
                    )
                })?),
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
                    sql_query::<Uuid>("select organization_id from nodes where id = ")
                        .bind(convergence.node_id.as_uuid())
                        .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?;
                if organization_id != convergence.organization_id.as_uuid() {
                    return Err(RepositoryError::NotFound.into());
                }
                let (last_issued_revision, installed_revision, aggregate_version) =
                    fetch_optional::<(u64, Option<u64>, u64), _>(
                        transaction,
                        sql_query::<(u64, Option<u64>, u64)>(
                            "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                        )
                        .bind(convergence.node_id.as_uuid())
                        .append(" for update"),
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
                if fetch_optional::<i32, _>(
                    transaction,
                    sql_query::<i32>(
                        "select 1 from gateway_publications where node_id = ",
                    )
                    .bind(convergence.node_id.as_uuid())
                    .append(" and state = 'pending' for update"),
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
                    sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                        .append(" where id = ")
                        .bind(convergence.previous_certificate_id.as_uuid())
                        .append(" for update"),
                )
                .await?
                .ok_or(RepositoryError::NotFound)?
                .certificate()?;
                if previous.organization_id != convergence.organization_id
                    || previous.node_id != convergence.node_id
                    || Some(previous.gateway_revision) != scope.installed_revision
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
                    sql_query::<RouteRow>(SELECT_ROUTES)
                        .append(" where gateway_node_id = ")
                        .bind(convergence.node_id.as_uuid())
                        .append(" and state = 'active' order by id for update"),
                )
                .await?
                .into_iter()
                .map(RouteRow::route)
                .collect::<Result<Vec<_>, _>>()?;
                validate_convergence_routes(transaction, convergence, &active).await?;
                if let Some(certificate) = &bundle.certificate {
                    validate_replacement_claims(convergence, certificate, &active)?;
                }

                insert_publication(transaction, &bundle.publication).await?;
                if let Some(certificate) = &bundle.certificate {
                    insert_certificate(transaction, certificate).await?;
                }
                insert_convergence(transaction, convergence).await?;
                require_one_row(
                    "Gateway certificate convergence scope",
                    execute(
                        transaction,
                        sql_query::<()>("update gateway_scopes set last_issued_revision = ")
                            .bind(bundle.publication.revision)
                            .append(", aggregate_version = aggregate_version + 1, updated_at = ")
                            .bind(bundle.publication.command_issued_at)
                            .append(" where node_id = ")
                            .bind(bundle.publication.node_id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(scope.aggregate_version),
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
            sql_query::<ConvergenceRow>(SELECT_CONVERGENCES)
                .append(" where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and gateway_revision = ")
                .bind(gateway_revision),
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
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<(Uuid, Uuid)>(
                "select certificate.node_id, certificate.id from gateway_certificates certificate join gateway_scopes scope on scope.node_id = certificate.node_id where certificate.state = 'ready' and scope.installed_revision > certificate.gateway_revision and not exists (select 1 from routes route where route.state = 'active' and route.gateway_certificate_id = certificate.id) order by certificate.gateway_revision, certificate.node_id, certificate.id limit ",
            )
            .bind(u64::try_from(limit).map_err(|_| {
                RepositoryError::Conflict(
                    "Gateway certificate revocation limit exceeds supported range".into(),
                )
            })?),
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
        sql_query::<ConvergenceRow>(SELECT_CONVERGENCES)
            .append(" where node_id = ")
            .bind(node_id)
            .append(" and gateway_revision = ")
            .bind(gateway_revision)
            .append(" and gateway_command_id = ")
            .bind(gateway_command_id)
            .append(" for update"),
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
            sql_query::<()>("update gateway_certificate_convergences set state = ")
                .bind(convergence.state.as_str())
                .append(", failure = ")
                .bind(convergence.failure.as_deref())
                .append(", acknowledged_at = ")
                .bind(convergence.acknowledged_at)
                .append(" where node_id = ")
                .bind(convergence.node_id.as_uuid())
                .append(" and gateway_revision = ")
                .bind(convergence.gateway_revision)
                .append(" and state = 'pending'"),
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
        sql_query::<RouteRow>(SELECT_ROUTES)
            .append(" where gateway_node_id = ")
            .bind(node_id.as_uuid())
            .append(" and state = 'active' order by id for update"),
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
                sql_query::<(u64, Option<u64>, u64)>(
                    "select last_issued_revision, installed_revision, aggregate_version from gateway_scopes where node_id = ",
                )
                .bind(node_id.as_uuid()),
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
    let certificate = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                .append(" where node_id = ")
                .bind(node_id.as_uuid())
                .append(" and gateway_revision = ")
                .bind(installed_revision.ok_or_else(|| {
                    RepositoryError::Storage(
                        "Gateway convergence scope has no installed revision".into(),
                    )
                })?),
        )
        .await
        .map_err(storage)?
        .ok_or_else(|| {
            RepositoryError::Storage("installed Gateway certificate disappeared".into())
        })?
        .certificate()?;
    let routes = query_routes(
        executor,
        sql_query::<RouteRow>(SELECT_ROUTES)
            .append(" where gateway_node_id = ")
            .bind(node_id.as_uuid())
            .append(" and state = 'active' order by id"),
    )
    .await?;
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
            sql_query::<String>("select state from domain_claims where id = ")
                .bind(claim_id.as_uuid()),
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
            sql_query::<PublicationRow>(SELECT_PUBLICATIONS)
                .append(" where node_id = ")
                .bind(convergence.node_id.as_uuid())
                .append(" and revision = ")
                .bind(convergence.gateway_revision),
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
            sql_query::<String>("select state from domain_claims where id = ")
                .bind(claim_id.as_uuid())
                .append(" for update"),
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
            sql_query::<()>(
                "insert into gateway_certificate_convergences (organization_id, node_id, gateway_revision, gateway_command_id, previous_certificate_id, replacement_certificate_id, snapshot_digest, retained_routes, rejected_routes, reason, state, failure, staged_at, acknowledged_at) values (",
            )
            .bind(convergence.organization_id.as_uuid())
            .append(", ")
            .bind(convergence.node_id.as_uuid())
            .append(", ")
            .bind(convergence.gateway_revision)
            .append(", ")
            .bind(convergence.gateway_command_id.as_uuid())
            .append(", ")
            .bind(convergence.previous_certificate_id.as_uuid())
            .append(", ")
            .bind(
                convergence
                    .replacement_certificate_id
                    .map(|certificate_id| certificate_id.as_uuid()),
            )
            .append(", ")
            .bind(convergence.snapshot_digest.as_str())
            .append(", ")
            .bind(retained_routes)
            .append(", ")
            .bind(rejected_routes)
            .append(", ")
            .bind(convergence.reason.as_str())
            .append(", ")
            .bind(convergence.state.as_str())
            .append(", ")
            .bind(convergence.failure.as_deref())
            .append(", ")
            .bind(convergence.staged_at)
            .append(", ")
            .bind(convergence.acknowledged_at)
            .append(")"),
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
    for version in &convergence.retained_routes {
        let mut route = lock_active_route(transaction, version).await?;
        let expected_version = route.aggregate_version;
        route
            .bind_gateway_certificate(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                convergence.replacement_certificate_id.ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "retained convergence route has no replacement certificate".into(),
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
        sql_query::<RouteRow>(SELECT_ROUTES)
            .append(" where id = ")
            .bind(version.route_id.as_uuid())
            .append(" for update"),
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
    require_one_row(
        "Gateway certificate route convergence",
        execute(
            transaction,
            sql_query::<()>("update routes set state = ")
                .bind(route.state.as_str())
                .append(", gateway_revision = ")
                .bind(route.gateway_revision)
                .append(", gateway_command_id = ")
                .bind(
                    route
                        .gateway_command_id
                        .map(|command_id| command_id.as_uuid()),
                )
                .append(", snapshot_digest = ")
                .bind(route.snapshot_digest.as_deref())
                .append(", gateway_certificate_id = ")
                .bind(
                    route
                        .gateway_certificate_id
                        .map(|certificate_id| certificate_id.as_uuid()),
                )
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
                .bind(expected_version),
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
