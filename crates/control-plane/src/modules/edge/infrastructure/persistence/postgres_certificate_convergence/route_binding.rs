use super::support::update_route;
use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn bind_active_routes_to_certificate(
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
    super::super::postgres_rollout_routes::bind_active_to_certificate(
        transaction,
        node_id,
        revision,
        command_id,
        snapshot_digest,
        certificate_id,
        acknowledged_at,
    )
    .await?;
    Ok(())
}
