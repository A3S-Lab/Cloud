use super::postgres::{PublicationRow, PublicationSelection, RouteRow, RouteSelection};
use super::postgres_certificate_convergence;
use super::postgres_cutovers;
use super::postgres_mcp_gateway_snapshots;
use super::postgres_rollout_routes;
use super::postgres_rollouts;
use super::postgres_schema::{
    GatewayCertificates, GatewayPublications, GatewayRouteProjections, GatewayScopes, Routes,
};
use super::postgres_tls::{update_certificate, CertificateRow, CertificateSelection};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::GatewayPublicationState;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use a3s_orm::{exists, not, select_from, update_table, PostgresExecutor};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) async fn project(
    executor: &PostgresExecutor,
    acknowledgement: &NodeGatewayAck,
    received_at: DateTime<Utc>,
) -> Result<bool, RepositoryError> {
    let mut acknowledgement = acknowledgement.clone();
    acknowledgement.acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
    let received_at = canonical_timestamp(received_at);
    acknowledgement
        .validate()
        .map_err(RepositoryError::Conflict)?;
    if received_at < acknowledgement.acknowledged_at {
        return Err(RepositoryError::Conflict(
            "Gateway acknowledgement receipt predates its node timestamp".into(),
        ));
    }
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = fetch_optional::<PublicationRow, _>(
                    transaction,
                    select_from::<GatewayPublications>()
                        .select(PublicationSelection)
                        .filter(GatewayPublications::node_id().eq(acknowledgement.node_id))
                        .filter(GatewayPublications::command_id().eq(acknowledgement.command_id))
                        .for_update(),
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
                if let Some((organization_id, mut rollout)) =
                    postgres_rollouts::lock_by_gateway_identity(
                        transaction,
                        acknowledgement.node_id,
                        acknowledgement.revision,
                        acknowledgement.command_id,
                    )
                    .await?
                {
                    let expected_rollout_version = rollout.aggregate_version;
                    if !rollout
                        .acknowledge(&acknowledgement)
                        .map_err(RepositoryError::Conflict)?
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "pending Gateway publication replayed a terminal rollout result".into(),
                        ));
                    }
                    let expected_certificate_id = rollout
                        .replicas
                        .iter()
                        .find(|replica| {
                            replica.node_id.as_uuid() == acknowledgement.node_id
                                && replica.revision == acknowledgement.revision
                                && replica.command_id.as_uuid() == acknowledgement.command_id
                        })
                        .and_then(|replica| replica.gateway_certificate_id);
                    let certificate_valid_at = match acknowledgement.state {
                        GatewayAckState::Applied => acknowledgement.acknowledged_at,
                        GatewayAckState::Rejected => rollout.started_at,
                    };
                    let certificate = postgres_rollouts::lock_certificate_binding(
                        transaction,
                        organization_id,
                        &rollout,
                        &publication,
                        certificate_valid_at,
                    )
                    .await?;
                    let (certificate, certificate_version) = match certificate {
                        Some((mut certificate, false)) => {
                            let version = certificate.aggregate_version;
                            certificate
                                .apply_gateway_acknowledgement(&acknowledgement)
                                .map_err(RepositoryError::Conflict)?;
                            (Some(certificate), Some(version))
                        }
                        Some((_, true)) | None => (None, None),
                    };
                    persist_publication_acknowledgement(transaction, &publication).await?;
                    if let (Some(certificate), Some(certificate_version)) =
                        (&certificate, certificate_version)
                    {
                        update_certificate(transaction, certificate, certificate_version).await?;
                    }
                    postgres_rollout_routes::project_acknowledgement(
                        transaction,
                        &rollout,
                        &acknowledgement,
                    )
                    .await?;
                    if acknowledgement.state == GatewayAckState::Applied {
                        if let Some(certificate_id) = expected_certificate_id {
                            postgres_certificate_convergence::bind_active_routes_to_certificate(
                                transaction,
                                NodeId::from_uuid(acknowledgement.node_id),
                                acknowledgement.revision,
                                NodeCommandId::from_uuid(acknowledgement.command_id),
                                &acknowledgement.snapshot_digest,
                                certificate_id,
                                acknowledgement.acknowledged_at,
                            )
                            .await?;
                        } else if has_active_routes(
                            transaction,
                            NodeId::from_uuid(acknowledgement.node_id),
                        )
                        .await?
                        {
                            return Err(PostgresPersistenceError::Invariant(
                                "certificate-free Gateway rollout retained active routes".into(),
                            ));
                        }
                    }
                    postgres_rollouts::persist_acknowledgement(
                        transaction,
                        &rollout,
                        NodeId::from_uuid(acknowledgement.node_id),
                        expected_rollout_version,
                    )
                    .await?;
                    if acknowledgement.state == GatewayAckState::Applied {
                        persist_installed_scope_revision(
                            transaction,
                            &publication,
                            acknowledgement.acknowledged_at,
                        )
                        .await?;
                    }
                    return Ok(true);
                }
                if let Some(marker) =
                    postgres_mcp_gateway_snapshots::lock_marker_by_gateway_identity(
                        transaction,
                        acknowledgement.node_id,
                        acknowledgement.revision,
                        acknowledgement.command_id,
                    )
                    .await?
                {
                    marker.validate_for(&publication).map_err(|error| {
                        PostgresPersistenceError::Invariant(format!(
                            "MCP Gateway acknowledgement identity is invalid: {error}"
                        ))
                    })?;
                    let certificate_rows = fetch_all::<CertificateRow, _>(
                        transaction,
                        select_from::<GatewayCertificates>()
                            .select(CertificateSelection)
                            .filter(GatewayCertificates::node_id().eq(acknowledgement.node_id))
                            .filter(
                                GatewayCertificates::gateway_revision()
                                    .eq(acknowledgement.revision),
                            )
                            .filter(
                                GatewayCertificates::gateway_command_id()
                                    .eq(acknowledgement.command_id),
                            )
                            .for_update(),
                    )
                    .await?;
                    let mut certificates = certificate_rows
                        .into_iter()
                        .map(CertificateRow::certificate)
                        .collect::<Result<Vec<_>, _>>()?;
                    let expected_certificate_id =
                        publication.certificate_request.as_ref().map(|request| {
                            crate::modules::shared_kernel::domain::GatewayCertificateId::from_uuid(
                                request.certificate_id,
                            )
                        });
                    if certificates.len() != usize::from(expected_certificate_id.is_some())
                        || certificates.first().map(|certificate| certificate.id)
                            != expected_certificate_id
                        || certificates.first().is_some_and(|certificate| {
                            certificate.organization_id != marker.organization_id
                                || certificate.node_id != marker.node_id
                                || certificate.gateway_revision != marker.gateway_revision
                                || certificate.gateway_command_id != marker.gateway_command_id
                                || certificate.snapshot_digest != marker.snapshot_digest
                        })
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "MCP Gateway acknowledgement certificate projection is inconsistent"
                                .into(),
                        ));
                    }
                    let mut certificate = certificates.pop();
                    let certificate_version = certificate
                        .as_ref()
                        .map(|certificate| certificate.aggregate_version);
                    if let Some(certificate) = &mut certificate {
                        certificate
                            .apply_gateway_acknowledgement(&acknowledgement)
                            .map_err(RepositoryError::Conflict)?;
                    }
                    persist_publication_acknowledgement(transaction, &publication).await?;
                    if let (Some(certificate), Some(certificate_version)) =
                        (&certificate, certificate_version)
                    {
                        update_certificate(transaction, certificate, certificate_version).await?;
                    }
                    if acknowledgement.state == GatewayAckState::Applied {
                        if let Some(certificate_id) = expected_certificate_id {
                            postgres_certificate_convergence::bind_active_routes_to_certificate(
                                transaction,
                                NodeId::from_uuid(acknowledgement.node_id),
                                acknowledgement.revision,
                                NodeCommandId::from_uuid(acknowledgement.command_id),
                                &acknowledgement.snapshot_digest,
                                certificate_id,
                                acknowledgement.acknowledged_at,
                            )
                            .await?;
                        } else if has_active_routes(
                            transaction,
                            NodeId::from_uuid(acknowledgement.node_id),
                        )
                        .await?
                        {
                            return Err(PostgresPersistenceError::Invariant(
                                "certificate-free MCP Gateway snapshot retained active routes"
                                    .into(),
                            ));
                        }
                        persist_installed_scope_revision(
                            transaction,
                            &publication,
                            acknowledgement.acknowledged_at,
                        )
                        .await?;
                    }
                    return Ok(true);
                }
                let certificate_rows = fetch_all::<CertificateRow, _>(
                    transaction,
                    select_from::<GatewayCertificates>()
                        .select(CertificateSelection)
                        .filter(GatewayCertificates::node_id().eq(acknowledgement.node_id))
                        .filter(
                            GatewayCertificates::gateway_revision().eq(acknowledgement.revision),
                        )
                        .filter(
                            GatewayCertificates::gateway_command_id()
                                .eq(acknowledgement.command_id),
                        )
                        .for_update(),
                )
                .await?;
                let rows = fetch_all::<RouteRow, _>(
                    transaction,
                    select_from::<Routes>()
                        .select(RouteSelection)
                        .filter(Routes::gateway_node_id().eq(acknowledgement.node_id))
                        .filter(Routes::gateway_revision().eq(acknowledgement.revision))
                        .filter(Routes::gateway_command_id().eq(acknowledgement.command_id))
                        .for_update(),
                )
                .await?;
                let mut cutover = postgres_cutovers::lock_by_gateway_identity(
                    transaction,
                    acknowledgement.node_id,
                    acknowledgement.revision,
                    acknowledgement.command_id,
                )
                .await?;
                let mut convergence = postgres_certificate_convergence::lock_by_gateway_identity(
                    transaction,
                    acknowledgement.node_id,
                    acknowledgement.revision,
                    acknowledgement.command_id,
                )
                .await?;
                let publication_kinds = usize::from(!rows.is_empty())
                    + usize::from(cutover.is_some())
                    + usize::from(convergence.is_some());
                if publication_kinds != 1 {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway publication must select one route publication kind".into(),
                    ));
                }
                let mut certificates = certificate_rows
                    .into_iter()
                    .map(CertificateRow::certificate)
                    .collect::<Result<Vec<_>, _>>()?;
                let staged_certificate_id = match &convergence {
                    Some(convergence) => convergence.replacement_certificate_id,
                    None if certificates.len() == 1 => Some(certificates[0].id),
                    None => {
                        return Err(PostgresPersistenceError::Invariant(
                            "Gateway publication must have exactly one staged certificate".into(),
                        ));
                    }
                };
                let active_certificate_id = convergence
                    .as_ref()
                    .and_then(|convergence| convergence.active_certificate_id())
                    .or(staged_certificate_id);
                if certificates.len() != usize::from(staged_certificate_id.is_some())
                    || certificates.first().map(|certificate| certificate.id)
                        != staged_certificate_id
                {
                    return Err(PostgresPersistenceError::Invariant(
                        "Gateway publication has inconsistent staged certificate material".into(),
                    ));
                }
                let mut certificate = certificates.pop();
                let certificate_version = certificate
                    .as_ref()
                    .map(|certificate| certificate.aggregate_version);
                if let Some(certificate) = &mut certificate {
                    certificate
                        .apply_gateway_acknowledgement(&acknowledgement)
                        .map_err(RepositoryError::Conflict)?;
                }
                let mut routes = rows
                    .into_iter()
                    .map(RouteRow::route)
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(convergence) = &mut convergence {
                    convergence
                        .acknowledge(&acknowledgement)
                        .map_err(RepositoryError::Conflict)?;
                } else if let Some(cutover) = &mut cutover {
                    cutover
                        .acknowledge(&acknowledgement)
                        .map_err(RepositoryError::Conflict)?;
                } else {
                    for route in &mut routes {
                        route
                            .apply_gateway_acknowledgement(&acknowledgement)
                            .map_err(RepositoryError::Conflict)?;
                    }
                }
                persist_publication_acknowledgement(transaction, &publication).await?;
                if let (Some(certificate), Some(certificate_version)) =
                    (&certificate, certificate_version)
                {
                    update_certificate(transaction, certificate, certificate_version).await?;
                }
                if let Some(convergence) = convergence {
                    postgres_certificate_convergence::persist_acknowledgement(
                        transaction,
                        &convergence,
                    )
                    .await?;
                } else if let Some(cutover) = cutover {
                    postgres_cutovers::persist_acknowledgement(transaction, &cutover).await?;
                } else {
                    for route in routes {
                        let expected_version =
                            route.aggregate_version.checked_sub(1).ok_or_else(|| {
                                PostgresPersistenceError::Invariant(
                                    "route acknowledgement version underflowed".into(),
                                )
                            })?;
                        require_one_row(
                            "route Gateway acknowledgement",
                            execute(
                                transaction,
                                update_table::<Routes>()
                                    .set(Routes::state(), route.state.as_str())
                                    .set(Routes::failure(), route.failure.clone())
                                    .set(Routes::aggregate_version(), route.aggregate_version)
                                    .set(Routes::updated_at(), route.updated_at)
                                    .set(Routes::activated_at(), route.activated_at)
                                    .filter(Routes::id().eq(route.id.as_uuid()))
                                    .filter(Routes::aggregate_version().eq(expected_version)),
                            )
                            .await?,
                        )?;
                    }
                }
                if acknowledgement.state == GatewayAckState::Applied {
                    if let Some(certificate_id) = active_certificate_id {
                        postgres_certificate_convergence::bind_active_routes_to_certificate(
                            transaction,
                            NodeId::from_uuid(acknowledgement.node_id),
                            acknowledgement.revision,
                            NodeCommandId::from_uuid(acknowledgement.command_id),
                            &acknowledgement.snapshot_digest,
                            certificate_id,
                            acknowledgement.acknowledged_at,
                        )
                        .await?;
                    } else if has_active_routes(
                        transaction,
                        NodeId::from_uuid(acknowledgement.node_id),
                    )
                    .await?
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "certificate-free Gateway snapshot retained active routes".into(),
                        ));
                    }
                    persist_installed_scope_revision(
                        transaction,
                        &publication,
                        acknowledgement.acknowledged_at,
                    )
                    .await?;
                }
                Ok(true)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn has_active_routes(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
) -> Result<bool, PostgresPersistenceError> {
    let projected_route = exists(
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::route_id())
            .filter(GatewayRouteProjections::route_id().eq_column(Routes::id())),
    );
    if fetch_optional::<Uuid, _>(
        transaction,
        select_from::<Routes>()
            .select(Routes::id())
            .filter(Routes::gateway_node_id().eq(node_id.as_uuid()))
            .filter(Routes::state().eq("active"))
            .filter(not(projected_route))
            .limit(1),
    )
    .await?
    .is_some()
    {
        return Ok(true);
    }
    postgres_rollout_routes::has_active(transaction, node_id).await
}

async fn persist_publication_acknowledgement(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &crate::modules::edge::domain::GatewayPublication,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Gateway publication acknowledgement",
        execute(
            transaction,
            update_table::<GatewayPublications>()
                .set(GatewayPublications::state(), publication.state.as_str())
                .set(GatewayPublications::failure(), publication.failure.clone())
                .set(
                    GatewayPublications::acknowledged_at(),
                    publication.acknowledged_at,
                )
                .filter(GatewayPublications::node_id().eq(publication.node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(publication.revision))
                .filter(GatewayPublications::state().eq("pending")),
        )
        .await?,
    )?;
    Ok(())
}

async fn persist_installed_scope_revision(
    transaction: &a3s_orm::PostgresTransaction,
    publication: &crate::modules::edge::domain::GatewayPublication,
    acknowledged_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let (installed_revision, aggregate_version) = fetch_optional::<(Option<u64>, u64), _>(
        transaction,
        select_from::<GatewayScopes>()
            .select((
                GatewayScopes::installed_revision(),
                GatewayScopes::aggregate_version(),
            ))
            .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "installed Gateway scope revision has no physical scope".into(),
        )
    })?;
    if installed_revision != publication.expected_revision {
        return Err(PostgresPersistenceError::Invariant(
            "installed Gateway scope revision changed before acknowledgement".into(),
        ));
    }
    let next_version = aggregate_version.checked_add(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "installed Gateway scope aggregate version overflowed".into(),
        )
    })?;
    require_one_row(
        "installed Gateway scope revision",
        execute(
            transaction,
            update_table::<GatewayScopes>()
                .set(
                    GatewayScopes::installed_revision(),
                    Some(publication.revision),
                )
                .set(GatewayScopes::aggregate_version(), next_version)
                .set(GatewayScopes::updated_at(), acknowledged_at)
                .filter(GatewayScopes::node_id().eq(publication.node_id.as_uuid()))
                .filter(GatewayScopes::aggregate_version().eq(aggregate_version)),
        )
        .await?,
    )?;
    Ok(())
}
