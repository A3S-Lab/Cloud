use super::postgres::{PublicationRow, RouteRow, SELECT_PUBLICATIONS, SELECT_ROUTES};
use super::postgres_certificate_convergence;
use super::postgres_cutovers;
use super::postgres_tls::{update_certificate, CertificateRow, SELECT_CERTIFICATES};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::GatewayPublicationState;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck};
use a3s_orm::{sql_query, PostgresExecutor};
use chrono::{DateTime, Utc};

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
                let certificate_rows = fetch_all::<CertificateRow, _>(
                    transaction,
                    sql_query::<CertificateRow>(SELECT_CERTIFICATES)
                        .append(" where node_id = ")
                        .bind(acknowledgement.node_id)
                        .append(" and gateway_revision = ")
                        .bind(acknowledgement.revision)
                        .append(" and gateway_command_id = ")
                        .bind(acknowledgement.command_id)
                        .append(" for update"),
                )
                .await?;
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
                                    .bind(expected_version),
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
                    } else if fetch_optional::<i32, _>(
                        transaction,
                        sql_query::<i32>("select 1 from routes where gateway_node_id = ")
                            .bind(acknowledgement.node_id)
                            .append(" and state = 'active' limit 1"),
                    )
                    .await?
                    .is_some()
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "certificate-free Gateway snapshot retained active routes".into(),
                        ));
                    }
                    require_one_row(
                        "installed Gateway scope revision",
                        execute(
                            transaction,
                            sql_query::<()>("update gateway_scopes set installed_revision = ")
                                .bind(acknowledgement.revision)
                                .append(
                                    ", aggregate_version = aggregate_version + 1, updated_at = ",
                                )
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
