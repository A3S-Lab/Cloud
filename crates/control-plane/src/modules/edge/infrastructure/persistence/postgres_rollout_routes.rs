use super::postgres::{RouteRow, RouteSelection};
use super::postgres_schema::{GatewayRouteOwnership, GatewayRouteProjections, Routes};
use crate::infrastructure::{
    execute, fetch_all, fetch_optional, is_unique_violation, require_one_row,
    PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::StageGatewayRollout;
use crate::modules::edge::domain::{GatewayRollout, Route};
use crate::modules::shared_kernel::domain::{
    GatewayCertificateId, GatewayRolloutId, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::NodeGatewayAck;
use a3s_orm::expression::Selection;
use a3s_orm::{
    delete_from, insert_into, select_from, update_table, Database, Expression, OrderDirection,
    PostgresDialect, PostgresExecutor,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct ProjectionRouteSelection;

impl Selection for ProjectionRouteSelection {
    type Output = RouteRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            GatewayRouteProjections::route_id().expression(),
            GatewayRouteProjections::organization_id().expression(),
            GatewayRouteProjections::project_id().expression(),
            GatewayRouteProjections::environment_id().expression(),
            GatewayRouteProjections::gateway_scope_id().expression(),
            GatewayRouteProjections::gateway_node_id().expression(),
            GatewayRouteProjections::hostname().expression(),
            GatewayRouteProjections::path_prefix().expression(),
            GatewayRouteProjections::workload_id().expression(),
            GatewayRouteProjections::workload_revision_id().expression(),
            GatewayRouteProjections::runtime_unit_id().expression(),
            GatewayRouteProjections::runtime_generation().expression(),
            GatewayRouteProjections::port_name().expression(),
            GatewayRouteProjections::upstream_origin().expression(),
            GatewayRouteProjections::target_observed_at().expression(),
            GatewayRouteProjections::state().expression(),
            GatewayRouteProjections::gateway_revision().expression(),
            GatewayRouteProjections::gateway_command_id().expression(),
            GatewayRouteProjections::snapshot_digest().expression(),
            GatewayRouteProjections::failure().expression(),
            GatewayRouteProjections::aggregate_version().expression(),
            GatewayRouteProjections::created_at().expression(),
            GatewayRouteProjections::updated_at().expression(),
            GatewayRouteProjections::activated_at().expression(),
            GatewayRouteProjections::domain_claim_id().expression(),
            GatewayRouteProjections::domain_pattern().expression(),
            GatewayRouteProjections::gateway_certificate_id().expression(),
        ]
    }
}

pub(super) async fn insert(
    transaction: &a3s_orm::PostgresTransaction,
    bundle: &StageGatewayRollout,
    route: &Route,
) -> Result<(), PostgresPersistenceError> {
    let gateway_revision = route.gateway_revision.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "staged Gateway Route projection omitted its Gateway revision".into(),
        )
    })?;
    let gateway_command_id = route.gateway_command_id.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "staged Gateway Route projection omitted its Gateway command".into(),
        )
    })?;
    let snapshot_digest = route.snapshot_digest.as_deref().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "staged Gateway Route projection omitted its snapshot digest".into(),
        )
    })?;
    let inserted = execute(
        transaction,
        insert_into::<GatewayRouteProjections>()
            .value(
                GatewayRouteProjections::gateway_rollout_id(),
                bundle.rollout.id.as_uuid(),
            )
            .value(GatewayRouteProjections::route_id(), route.id.as_uuid())
            .value(
                GatewayRouteProjections::gateway_scope_id(),
                route.gateway_scope_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::membership_generation(),
                bundle.rollout.membership_generation,
            )
            .value(
                GatewayRouteProjections::organization_id(),
                route.organization_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::project_id(),
                route.project_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::environment_id(),
                route.environment_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::gateway_node_id(),
                route.gateway_node_id.as_uuid(),
            )
            .value(GatewayRouteProjections::hostname(), route.hostname.as_str())
            .value(
                GatewayRouteProjections::path_prefix(),
                route.path_prefix.as_str(),
            )
            .value(
                GatewayRouteProjections::workload_id(),
                route.workload_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::workload_revision_id(),
                route.target.workload_revision_id.as_uuid(),
            )
            .value(
                GatewayRouteProjections::runtime_unit_id(),
                route.target.runtime_unit_id.as_str(),
            )
            .value(
                GatewayRouteProjections::runtime_generation(),
                route.target.runtime_generation,
            )
            .value(
                GatewayRouteProjections::port_name(),
                route.target.port_name.as_str(),
            )
            .value(
                GatewayRouteProjections::upstream_origin(),
                route.target.upstream.as_str(),
            )
            .value(
                GatewayRouteProjections::target_observed_at(),
                route.target.observed_at,
            )
            .value(GatewayRouteProjections::state(), route.state.as_str())
            .value(
                GatewayRouteProjections::gateway_revision(),
                gateway_revision,
            )
            .value(
                GatewayRouteProjections::gateway_command_id(),
                gateway_command_id.as_uuid(),
            )
            .value(GatewayRouteProjections::snapshot_digest(), snapshot_digest)
            .value(GatewayRouteProjections::failure(), route.failure.clone())
            .value(
                GatewayRouteProjections::aggregate_version(),
                route.aggregate_version,
            )
            .value(GatewayRouteProjections::created_at(), route.created_at)
            .value(GatewayRouteProjections::updated_at(), route.updated_at)
            .value(GatewayRouteProjections::activated_at(), route.activated_at)
            .value(
                GatewayRouteProjections::domain_claim_id(),
                route.domain_claim_id.map(|id| id.as_uuid()),
            )
            .value(
                GatewayRouteProjections::domain_pattern(),
                route
                    .domain_pattern
                    .as_ref()
                    .map(|pattern| pattern.as_str().to_owned()),
            )
            .value(
                GatewayRouteProjections::gateway_certificate_id(),
                route.gateway_certificate_id.map(|id| id.as_uuid()),
            ),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("Gateway Route projection", rows)?,
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "hostname and path are already owned by this physical Gateway member".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }
    let ownership = execute(
        transaction,
        insert_into::<GatewayRouteOwnership>()
            .value(
                GatewayRouteOwnership::gateway_rollout_id(),
                bundle.rollout.id.as_uuid(),
            )
            .value(GatewayRouteOwnership::route_id(), route.id.as_uuid())
            .value(
                GatewayRouteOwnership::gateway_node_id(),
                route.gateway_node_id.as_uuid(),
            )
            .value(GatewayRouteOwnership::hostname(), route.hostname.as_str())
            .value(
                GatewayRouteOwnership::path_prefix(),
                route.path_prefix.as_str(),
            )
            .value(GatewayRouteOwnership::created_at(), route.created_at),
    )
    .await;
    match ownership {
        Ok(rows) => require_one_row("Gateway Route ownership", rows),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "hostname and path are already durably owned by this physical Gateway member".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn release_failed_ownership(
    transaction: &a3s_orm::PostgresTransaction,
    failed: &GatewayRollout,
) -> Result<(), PostgresPersistenceError> {
    let ownership = fetch_all::<(Uuid, Uuid), _>(
        transaction,
        select_from::<GatewayRouteOwnership>()
            .select((
                GatewayRouteOwnership::route_id(),
                GatewayRouteOwnership::gateway_node_id(),
            ))
            .filter(GatewayRouteOwnership::gateway_rollout_id().eq(failed.id.as_uuid()))
            .order_by(
                GatewayRouteOwnership::gateway_node_id(),
                OrderDirection::Asc,
            )
            .for_update(),
    )
    .await?;
    if ownership.is_empty() {
        return ensure_rollout_has_no_projections(transaction, failed.id)
            .await
            .map(|_| ());
    }
    let expected_nodes = failed
        .replicas
        .iter()
        .map(|replica| replica.node_id.as_uuid())
        .collect::<Vec<_>>();
    if ownership.len() != expected_nodes.len()
        || ownership
            .iter()
            .map(|(_, node_id)| *node_id)
            .ne(expected_nodes)
    {
        return Err(PostgresPersistenceError::Invariant(
            "failed Gateway rollout durable Route ownership is incomplete".into(),
        ));
    }
    let route_ids = ownership
        .iter()
        .map(|(route_id, _)| *route_id)
        .collect::<std::collections::BTreeSet<_>>();
    if route_ids.len() != 1 {
        return Err(PostgresPersistenceError::Invariant(
            "failed Gateway rollout ownership references inconsistent logical Routes".into(),
        ));
    }
    let route_id = *route_ids.iter().next().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "failed Gateway rollout ownership omitted its logical Route".into(),
        )
    })?;
    let logical_state = fetch_optional::<String, _>(
        transaction,
        select_from::<Routes>()
            .select(Routes::state())
            .filter(Routes::id().eq(route_id))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "failed Gateway rollout logical Route disappeared".into(),
        )
    })?;
    if logical_state != "rejected" {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollback cannot release ownership for a non-rejected logical Route".into(),
        ));
    }
    let deleted = execute(
        transaction,
        delete_from::<GatewayRouteOwnership>()
            .filter(GatewayRouteOwnership::gateway_rollout_id().eq(failed.id.as_uuid())),
    )
    .await?;
    if usize::try_from(deleted).ok() != Some(ownership.len()) {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollback did not release every failed physical Route ownership".into(),
        ));
    }
    Ok(())
}

pub(super) async fn release_route_ownership(
    transaction: &a3s_orm::PostgresTransaction,
    route_id: crate::modules::shared_kernel::domain::RouteId,
) -> Result<(), PostgresPersistenceError> {
    execute(
        transaction,
        delete_from::<GatewayRouteOwnership>()
            .filter(GatewayRouteOwnership::route_id().eq(route_id.as_uuid())),
    )
    .await?;
    Ok(())
}

pub(super) async fn project_acknowledgement(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    acknowledgement: &NodeGatewayAck,
) -> Result<bool, PostgresPersistenceError> {
    let Some(mut projection) = lock_member_projection(
        transaction,
        rollout.id,
        NodeId::from_uuid(acknowledgement.node_id),
    )
    .await?
    else {
        return ensure_rollout_has_no_projections(transaction, rollout.id).await;
    };
    let expected_projection_version = projection.aggregate_version;
    projection
        .apply_gateway_acknowledgement(acknowledgement)
        .map_err(RepositoryError::Conflict)?;
    persist_projection_transition(
        transaction,
        rollout.id,
        &projection,
        expected_projection_version,
    )
    .await?;
    converge_logical_route(transaction, rollout, &projection).await?;
    Ok(true)
}

pub(super) async fn project_unavailability(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    node_id: NodeId,
    failure: &str,
    observed_at: DateTime<Utc>,
) -> Result<bool, PostgresPersistenceError> {
    let Some(mut projection) = lock_member_projection(transaction, rollout.id, node_id).await?
    else {
        return ensure_rollout_has_no_projections(transaction, rollout.id).await;
    };
    let expected_projection_version = projection.aggregate_version;
    projection
        .mark_unavailable_from_gateway_rollout(failure, observed_at)
        .map_err(RepositoryError::Conflict)?;
    persist_projection_transition(
        transaction,
        rollout.id,
        &projection,
        expected_projection_version,
    )
    .await?;
    converge_logical_route(transaction, rollout, &projection).await?;
    Ok(true)
}

pub(super) async fn active(
    executor: &PostgresExecutor,
    node_id: NodeId,
) -> Result<Vec<Route>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayRouteProjections>()
                .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
                .select(ProjectionRouteSelection)
                .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
                .filter(GatewayRouteProjections::state().eq("active"))
                .filter(Routes::state().eq("active"))
                .order_by(GatewayRouteProjections::hostname(), OrderDirection::Asc)
                .order_by(GatewayRouteProjections::path_prefix(), OrderDirection::Asc)
                .order_by(GatewayRouteProjections::route_id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(RouteRow::route)
        .collect()
}

pub(super) async fn lock_active(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
) -> Result<Vec<Route>, PostgresPersistenceError> {
    fetch_all::<RouteRow, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .select(ProjectionRouteSelection)
            .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active"))
            .order_by(GatewayRouteProjections::route_id(), OrderDirection::Asc)
            .for_update(),
    )
    .await?
    .into_iter()
    .map(RouteRow::route)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn bind_route_to_certificate(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
    route_id: crate::modules::shared_kernel::domain::RouteId,
    revision: u64,
    command_id: NodeCommandId,
    snapshot_digest: &str,
    certificate_id: GatewayCertificateId,
    acknowledged_at: DateTime<Utc>,
) -> Result<bool, PostgresPersistenceError> {
    let Some((rollout_id, mut route)) =
        lock_projection_by_route_and_node(transaction, route_id, node_id).await?
    else {
        return Ok(false);
    };
    if route.state != crate::modules::edge::domain::RouteState::Active {
        return Ok(false);
    }
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
        persist_projection_binding(transaction, rollout_id, &route, expected_version).await?;
    }
    Ok(true)
}

pub(super) async fn reject_route_for_domain_revocation(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
    route_id: crate::modules::shared_kernel::domain::RouteId,
    revision: u64,
    command_id: NodeCommandId,
    snapshot_digest: &str,
    acknowledged_at: DateTime<Utc>,
) -> Result<bool, PostgresPersistenceError> {
    let Some((rollout_id, mut route)) =
        lock_projection_by_route_and_node(transaction, route_id, node_id).await?
    else {
        return Ok(false);
    };
    if route.state != crate::modules::edge::domain::RouteState::Active {
        return Err(RepositoryError::Conflict(
            "Gateway Route projection changed before domain revocation applied".into(),
        )
        .into());
    }
    let expected_version = route.aggregate_version;
    route
        .reject_for_domain_revocation(
            revision,
            command_id,
            snapshot_digest.into(),
            acknowledged_at,
        )
        .map_err(RepositoryError::Conflict)?;
    require_one_row(
        "active Gateway Route projection domain revocation",
        execute(
            transaction,
            update_table::<GatewayRouteProjections>()
                .set(GatewayRouteProjections::state(), route.state.as_str())
                .set(GatewayRouteProjections::gateway_revision(), revision)
                .set(
                    GatewayRouteProjections::gateway_command_id(),
                    command_id.as_uuid(),
                )
                .set(GatewayRouteProjections::snapshot_digest(), snapshot_digest)
                .set(GatewayRouteProjections::failure(), route.failure.clone())
                .set(
                    GatewayRouteProjections::aggregate_version(),
                    route.aggregate_version,
                )
                .set(GatewayRouteProjections::updated_at(), route.updated_at)
                .set(GatewayRouteProjections::activated_at(), route.activated_at)
                .filter(GatewayRouteProjections::gateway_rollout_id().eq(rollout_id.as_uuid()))
                .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
                .filter(GatewayRouteProjections::aggregate_version().eq(expected_version))
                .filter(GatewayRouteProjections::state().eq("active")),
        )
        .await?,
    )?;
    require_one_row(
        "physical Gateway Route ownership domain revocation",
        execute(
            transaction,
            delete_from::<GatewayRouteOwnership>()
                .filter(GatewayRouteOwnership::route_id().eq(route_id.as_uuid()))
                .filter(GatewayRouteOwnership::gateway_node_id().eq(node_id.as_uuid())),
        )
        .await?,
    )?;
    Ok(true)
}

pub(super) async fn has_active_route_projection(
    transaction: &a3s_orm::PostgresTransaction,
    route_id: crate::modules::shared_kernel::domain::RouteId,
) -> Result<bool, PostgresPersistenceError> {
    Ok(fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::gateway_node_id())
            .filter(GatewayRouteProjections::route_id().eq(route_id.as_uuid()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .limit(1),
    )
    .await?
    .is_some())
}

pub(super) async fn route_projection(
    transaction: &a3s_orm::PostgresTransaction,
    route_id: crate::modules::shared_kernel::domain::RouteId,
    node_id: NodeId,
) -> Result<Option<Route>, PostgresPersistenceError> {
    Ok(
        lock_projection_by_route_and_node(transaction, route_id, node_id)
            .await?
            .map(|(_, route)| route),
    )
}

async fn lock_projection_by_route_and_node(
    transaction: &a3s_orm::PostgresTransaction,
    route_id: crate::modules::shared_kernel::domain::RouteId,
    node_id: NodeId,
) -> Result<Option<(GatewayRolloutId, Route)>, PostgresPersistenceError> {
    let rollout_ids = fetch_all::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::gateway_rollout_id())
            .filter(GatewayRouteProjections::route_id().eq(route_id.as_uuid()))
            .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
            .order_by(
                GatewayRouteProjections::gateway_rollout_id(),
                OrderDirection::Asc,
            )
            .for_update(),
    )
    .await?;
    match rollout_ids.as_slice() {
        [] => Ok(None),
        [rollout_id] => {
            let rollout_id = GatewayRolloutId::from_uuid(*rollout_id);
            let route = lock_member_projection(transaction, rollout_id, node_id)
                .await?
                .ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "Gateway Route projection disappeared while locked".into(),
                    )
                })?;
            Ok(Some((rollout_id, route)))
        }
        _ => Err(PostgresPersistenceError::Invariant(
            "one logical Route has duplicate physical projections on a Gateway member".into(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn bind_active_to_certificate(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
    revision: u64,
    command_id: NodeCommandId,
    snapshot_digest: &str,
    certificate_id: GatewayCertificateId,
    acknowledged_at: DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let rollout_ids = crate::infrastructure::fetch_all::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .select(GatewayRouteProjections::gateway_rollout_id())
            .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active"))
            .order_by(
                GatewayRouteProjections::gateway_rollout_id(),
                OrderDirection::Asc,
            )
            .for_update(),
    )
    .await?;
    for rollout_id in rollout_ids {
        let rollout_id = GatewayRolloutId::from_uuid(rollout_id);
        let mut route = lock_member_projection(transaction, rollout_id, node_id)
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "active Gateway Route projection disappeared while rebinding".into(),
                )
            })?;
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
            persist_projection_binding(transaction, rollout_id, &route, expected_version).await?;
        }
    }
    Ok(())
}

pub(super) async fn has_active(
    transaction: &a3s_orm::PostgresTransaction,
    node_id: NodeId,
) -> Result<bool, PostgresPersistenceError> {
    Ok(fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .inner_join::<Routes>(GatewayRouteProjections::route_id().eq_column(Routes::id()))
            .select(GatewayRouteProjections::gateway_rollout_id())
            .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
            .filter(GatewayRouteProjections::state().eq("active"))
            .filter(Routes::state().eq("active"))
            .limit(1),
    )
    .await?
    .is_some())
}

async fn lock_member_projection(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
) -> Result<Option<Route>, PostgresPersistenceError> {
    fetch_optional::<RouteRow, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .select(ProjectionRouteSelection)
            .filter(GatewayRouteProjections::gateway_rollout_id().eq(rollout_id.as_uuid()))
            .filter(GatewayRouteProjections::gateway_node_id().eq(node_id.as_uuid()))
            .for_update(),
    )
    .await?
    .map(RouteRow::route)
    .transpose()
    .map_err(Into::into)
}

async fn ensure_rollout_has_no_projections(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
) -> Result<bool, PostgresPersistenceError> {
    if fetch_optional::<Uuid, _>(
        transaction,
        select_from::<GatewayRouteProjections>()
            .select(GatewayRouteProjections::gateway_node_id())
            .filter(GatewayRouteProjections::gateway_rollout_id().eq(rollout_id.as_uuid()))
            .limit(1),
    )
    .await?
    .is_some()
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway Route rollout omitted a member projection".into(),
        ));
    }
    Ok(false)
}

async fn persist_projection_transition(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
    route: &Route,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    require_one_row(
        "Gateway Route projection transition",
        execute(
            transaction,
            update_table::<GatewayRouteProjections>()
                .set(GatewayRouteProjections::state(), route.state.as_str())
                .set(GatewayRouteProjections::failure(), route.failure.clone())
                .set(
                    GatewayRouteProjections::aggregate_version(),
                    route.aggregate_version,
                )
                .set(GatewayRouteProjections::updated_at(), route.updated_at)
                .set(GatewayRouteProjections::activated_at(), route.activated_at)
                .filter(GatewayRouteProjections::gateway_rollout_id().eq(rollout_id.as_uuid()))
                .filter(
                    GatewayRouteProjections::gateway_node_id().eq(route.gateway_node_id.as_uuid()),
                )
                .filter(GatewayRouteProjections::aggregate_version().eq(expected_version))
                .filter(GatewayRouteProjections::state().eq("publishing")),
        )
        .await?,
    )
}

async fn persist_projection_binding(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
    route: &Route,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let revision = route.gateway_revision.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "active Gateway Route projection binding omitted its revision".into(),
        )
    })?;
    let command_id = route.gateway_command_id.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "active Gateway Route projection binding omitted its command".into(),
        )
    })?;
    let snapshot_digest = route.snapshot_digest.as_deref().ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "active Gateway Route projection binding omitted its snapshot digest".into(),
        )
    })?;
    require_one_row(
        "active Gateway Route projection binding",
        execute(
            transaction,
            update_table::<GatewayRouteProjections>()
                .set(GatewayRouteProjections::gateway_revision(), revision)
                .set(
                    GatewayRouteProjections::gateway_command_id(),
                    command_id.as_uuid(),
                )
                .set(GatewayRouteProjections::snapshot_digest(), snapshot_digest)
                .set(
                    GatewayRouteProjections::gateway_certificate_id(),
                    route
                        .gateway_certificate_id
                        .map(|certificate_id| certificate_id.as_uuid()),
                )
                .set(
                    GatewayRouteProjections::aggregate_version(),
                    route.aggregate_version,
                )
                .set(GatewayRouteProjections::updated_at(), route.updated_at)
                .filter(GatewayRouteProjections::gateway_rollout_id().eq(rollout_id.as_uuid()))
                .filter(
                    GatewayRouteProjections::gateway_node_id().eq(route.gateway_node_id.as_uuid()),
                )
                .filter(GatewayRouteProjections::aggregate_version().eq(expected_version))
                .filter(GatewayRouteProjections::state().eq("active")),
        )
        .await?,
    )
}

async fn converge_logical_route(
    transaction: &a3s_orm::PostgresTransaction,
    rollout: &GatewayRollout,
    projection: &Route,
) -> Result<(), PostgresPersistenceError> {
    let row = fetch_optional::<RouteRow, _>(
        transaction,
        select_from::<Routes>()
            .select(RouteSelection)
            .filter(Routes::id().eq(projection.id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Gateway Route rollout logical Route disappeared".into(),
        )
    })?;
    let mut logical = row.route()?;
    validate_logical_projection(&logical, projection, rollout)?;
    let observed_at = rollout_observed_at(rollout);
    let changed = if rollout
        .serves_traffic()
        .map_err(PostgresPersistenceError::Invariant)?
    {
        logical
            .activate_from_gateway_rollout(observed_at)
            .map_err(RepositoryError::Conflict)?
    } else if rollout.state.terminal() {
        logical
            .reject_from_gateway_rollout(
                "Gateway rollout did not reach its readiness threshold",
                observed_at,
            )
            .map_err(RepositoryError::Conflict)?
    } else {
        false
    };
    if !changed {
        return Ok(());
    }
    let expected_version = logical.aggregate_version.checked_sub(1).ok_or_else(|| {
        PostgresPersistenceError::Invariant("logical Route version underflowed".into())
    })?;
    require_one_row(
        "logical Route rollout transition",
        execute(
            transaction,
            update_table::<Routes>()
                .set(Routes::state(), logical.state.as_str())
                .set(Routes::failure(), logical.failure.clone())
                .set(Routes::aggregate_version(), logical.aggregate_version)
                .set(Routes::updated_at(), logical.updated_at)
                .set(Routes::activated_at(), logical.activated_at)
                .filter(Routes::id().eq(logical.id.as_uuid()))
                .filter(Routes::aggregate_version().eq(expected_version))
                .filter(Routes::state().eq("publishing")),
        )
        .await?,
    )
}

fn validate_logical_projection(
    logical: &Route,
    projection: &Route,
    rollout: &GatewayRollout,
) -> Result<(), PostgresPersistenceError> {
    if logical.id != projection.id
        || logical.organization_id != projection.organization_id
        || logical.project_id != projection.project_id
        || logical.environment_id != projection.environment_id
        || logical.gateway_scope_id != rollout.gateway_scope_id
        || projection.gateway_scope_id != rollout.gateway_scope_id
        || logical.hostname != projection.hostname
        || logical.path_prefix != projection.path_prefix
        || logical.domain_claim_id != projection.domain_claim_id
        || logical.domain_pattern != projection.domain_pattern
        || logical.workload_id != projection.workload_id
        || logical.target.workload_revision_id != projection.target.workload_revision_id
        || logical.target.runtime_unit_id != projection.target.runtime_unit_id
        || logical.target.runtime_generation != projection.target.runtime_generation
        || logical.target.port_name != projection.target.port_name
        || logical.created_at != projection.created_at
    {
        return Err(PostgresPersistenceError::Invariant(
            "Gateway rollout logical and physical Route projections diverged".into(),
        ));
    }
    Ok(())
}

fn rollout_observed_at(rollout: &GatewayRollout) -> DateTime<Utc> {
    rollout
        .replicas
        .iter()
        .filter_map(|replica| replica.acknowledged_at)
        .max()
        .unwrap_or(rollout.started_at)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
