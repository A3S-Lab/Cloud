use super::*;
use crate::modules::edge::infrastructure::persistence::{postgres_rollout_routes, postgres_tls};

pub(super) async fn load_target(
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
    let projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
    );
    let mut routes = query_routes(
        executor,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .filter(not(projected_route))
            .order_by(Routes::id(), OrderDirection::Asc),
    )
    .await?;
    routes.extend(postgres_rollout_routes::active(executor, node_id).await?);
    routes.sort_by_key(|route| route.id);
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
        postgres_tls::find_gateway_certificate(executor, node_id, certificate_id).await?;
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

pub(super) async fn load_result(
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
            postgres_tls::find_gateway_certificate(executor, convergence.node_id, certificate_id)
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

pub(super) async fn validate_convergence_routes(
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

pub(super) fn validate_active_certificate(
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

pub(super) fn validate_replacement_claims(
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

pub(super) async fn insert_convergence(
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

pub(super) async fn persist_route_convergence(
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
        let certificate_id = active_certificate_id.ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "retained convergence route has no active certificate".into(),
            )
        })?;
        if postgres_rollout_routes::bind_route_to_certificate(
            transaction,
            convergence.node_id,
            version.route_id,
            convergence.gateway_revision,
            convergence.gateway_command_id,
            &convergence.snapshot_digest,
            certificate_id,
            acknowledged_at,
        )
        .await?
        {
            let mut logical = lock_active_logical_route(transaction, version.route_id).await?;
            if logical.gateway_node_id == convergence.node_id {
                let expected_version = logical.aggregate_version;
                logical
                    .bind_gateway_certificate(
                        convergence.gateway_revision,
                        convergence.gateway_command_id,
                        convergence.snapshot_digest.clone(),
                        certificate_id,
                        acknowledged_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                update_route(transaction, &logical, expected_version).await?;
            }
            continue;
        }
        let mut route = lock_active_route(transaction, version).await?;
        let expected_version = route.aggregate_version;
        route
            .bind_gateway_certificate(
                convergence.gateway_revision,
                convergence.gateway_command_id,
                convergence.snapshot_digest.clone(),
                certificate_id,
                acknowledged_at,
            )
            .map_err(RepositoryError::Conflict)?;
        update_route(transaction, &route, expected_version).await?;
    }
    for version in &convergence.rejected_routes {
        if postgres_rollout_routes::reject_route_for_domain_revocation(
            transaction,
            convergence.node_id,
            version.route_id,
            convergence.gateway_revision,
            convergence.gateway_command_id,
            &convergence.snapshot_digest,
            acknowledged_at,
        )
        .await?
        {
            if !postgres_rollout_routes::has_active_route_projection(transaction, version.route_id)
                .await?
            {
                let mut logical = lock_active_logical_route(transaction, version.route_id).await?;
                let primary = postgres_rollout_routes::route_projection(
                    transaction,
                    version.route_id,
                    logical.gateway_node_id,
                )
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "replicated domain revocation lost its primary Route projection".into(),
                    )
                })?;
                if primary.state != RouteState::Rejected {
                    return Err(PostgresPersistenceError::Invariant(
                        "replicated domain revocation completed before its primary member".into(),
                    ));
                }
                let revision = primary.gateway_revision.ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "rejected primary Route projection omitted its revision".into(),
                    )
                })?;
                let command_id = primary.gateway_command_id.ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "rejected primary Route projection omitted its command".into(),
                    )
                })?;
                let snapshot_digest = primary.snapshot_digest.clone().ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "rejected primary Route projection omitted its digest".into(),
                    )
                })?;
                let expected_version = logical.aggregate_version;
                logical
                    .reject_for_domain_revocation(
                        revision,
                        command_id,
                        snapshot_digest,
                        acknowledged_at.max(primary.updated_at),
                    )
                    .map_err(RepositoryError::Conflict)?;
                update_route(transaction, &logical, expected_version).await?;
            }
            continue;
        }
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
        postgres_rollout_routes::release_route_ownership(transaction, route.id).await?;
    }
    Ok(())
}

async fn lock_active_logical_route(
    transaction: &PostgresTransaction,
    route_id: RouteId,
) -> Result<Route, PostgresPersistenceError> {
    let route = fetch_optional::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::id().eq(route_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .route()?;
    if route.state != RouteState::Active {
        return Err(RepositoryError::Conflict(
            "logical Route changed before replicated certificate convergence applied".into(),
        )
        .into());
    }
    Ok(route)
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

pub(super) async fn update_route(
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

pub(super) fn validate_scope(scope: &GatewayScopeState) -> Result<(), RepositoryError> {
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

pub(super) fn validate_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway certificate convergence batch limit is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| {
        RepositoryError::Storage(format!(
            "stored Gateway certificate convergence {label} is invalid: {error}"
        ))
    }
}

pub(super) fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

pub(super) fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}
