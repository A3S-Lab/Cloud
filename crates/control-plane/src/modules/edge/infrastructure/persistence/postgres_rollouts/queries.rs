use super::models::{
    rebuild_rollout, ReplicaRow, ReplicaSelection, RollbackRow, RollbackSelection,
    RolloutReplicaSelection, RolloutRow, RolloutSelection,
};
use crate::infrastructure::{
    fetch_all, fetch_optional, idempotency_replay, transaction_error, PostgresPersistenceError,
};
use crate::modules::edge::domain::repositories::{
    GatewayRolloutResult, GatewayRolloutRollbackTarget,
};
use crate::modules::edge::domain::{
    GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutRollback, GatewayRolloutRollbackState,
};
use crate::modules::edge::infrastructure::persistence::postgres_gateway_scopes;
use crate::modules::edge::infrastructure::persistence::postgres_schema::{
    GatewayRolloutReplicas, GatewayRolloutRollbacks, GatewayRollouts,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, GatewayScopeId, IdempotencyRequest, OrganizationId, RepositoryError,
};
use a3s_orm::function::{bound, coalesce, max, sql_function, TypedExpression};
use a3s_orm::{
    exists, not, select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor,
};
use uuid::Uuid;

pub(in super::super) async fn replay(
    executor: &PostgresExecutor,
    idempotency: &IdempotencyRequest,
) -> Result<Option<GatewayRolloutResult>, RepositoryError> {
    let idempotency = idempotency.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(mut replay) =
                    idempotency_replay::<GatewayRolloutResult>(transaction, &idempotency).await?
                else {
                    return Ok(None);
                };
                replay.value.replayed = true;
                Ok(Some(replay.value))
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in super::super) async fn next_generation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    gateway_scope_id: GatewayScopeId,
) -> Result<u64, RepositoryError> {
    let current = Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(
            select_from::<GatewayRollouts>()
                .select(coalesce::<u64>([
                    max(GatewayRollouts::generation()).expression(),
                    bound::<u64>(0_u64).expression(),
                ]))
                .filter(GatewayRollouts::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRollouts::gateway_scope_id().eq(gateway_scope_id.as_uuid())),
        )
        .await
        .map_err(storage)?;
    current.checked_add(1).ok_or_else(|| {
        RepositoryError::Conflict("Gateway rollout generation space is exhausted".into())
    })
}

pub(in super::super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
) -> Result<GatewayRollout, RepositoryError> {
    let database = Database::new(PostgresDialect, executor.clone());
    let rows = database
        .fetch_all_as(
            select_from::<GatewayRollouts>()
                .inner_join::<GatewayRolloutReplicas>(
                    GatewayRollouts::id().eq_column(GatewayRolloutReplicas::gateway_rollout_id()),
                )
                .select(RolloutReplicaSelection)
                .filter(GatewayRollouts::organization_id().eq(organization_id.as_uuid()))
                .filter(GatewayRollouts::id().eq(rollout_id.as_uuid()))
                .order_by(GatewayRolloutReplicas::node_id(), OrderDirection::Asc),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter();
    rebuild_rollout(rows)
}

pub(in super::super) async fn find_rollback(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    failed_rollout_id: GatewayRolloutId,
) -> Result<GatewayRolloutRollback, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayRolloutRollbacks>()
                .inner_join::<GatewayRollouts>(
                    GatewayRolloutRollbacks::failed_rollout_id().eq_column(GatewayRollouts::id()),
                )
                .select(RollbackSelection)
                .filter(GatewayRollouts::organization_id().eq(organization_id.as_uuid()))
                .filter(
                    GatewayRolloutRollbacks::failed_rollout_id().eq(failed_rollout_id.as_uuid()),
                ),
        )
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::NotFound)?
        .rollback()
}

pub(in super::super) async fn pending_rollbacks(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<GatewayRolloutRollbackTarget>, RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway rollback scan batch limit is invalid".into(),
        ));
    }
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("Gateway rollback scan limit exceeds supported range".into())
    })?;
    let unresolved_recovery = select_from::<GatewayRolloutReplicas>()
        .select(GatewayRolloutReplicas::node_id())
        .filter(
            GatewayRolloutReplicas::gateway_rollout_id()
                .eq_column(GatewayRolloutRollbacks::failed_rollout_id()),
        )
        .filter(GatewayRolloutReplicas::state().eq("unavailable"))
        .filter(
            GatewayRolloutReplicas::recovery()
                .is_null()
                .or(rollback_recovery_state().ne("observed")),
        );
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayRolloutRollbacks>()
                .inner_join::<GatewayRollouts>(
                    GatewayRolloutRollbacks::failed_rollout_id().eq_column(GatewayRollouts::id()),
                )
                .select((
                    GatewayRollouts::organization_id(),
                    GatewayRolloutRollbacks::failed_rollout_id(),
                ))
                .filter(GatewayRolloutRollbacks::state().eq("required"))
                .filter(not(exists(unresolved_recovery)))
                .order_by(GatewayRolloutRollbacks::required_at(), OrderDirection::Asc)
                .order_by(
                    GatewayRolloutRollbacks::failed_rollout_id(),
                    OrderDirection::Asc,
                )
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;
    let mut targets = Vec::with_capacity(identities.len());
    for (organization_id, failed_rollout_id) in identities {
        let organization_id = OrganizationId::from_uuid(organization_id);
        let failed_rollout_id = GatewayRolloutId::from_uuid(failed_rollout_id);
        let rollback = find_rollback(executor, organization_id, failed_rollout_id).await?;
        if rollback.state != GatewayRolloutRollbackState::Required {
            continue;
        }
        let failed_rollout = find(executor, organization_id, failed_rollout_id).await?;
        if failed_rollout.replicas.iter().any(|replica| {
            replica.state == GatewayReplicaRolloutState::Unavailable
                && replica.recovery.as_ref().is_none_or(|recovery| {
                    recovery.state
                        != crate::modules::edge::domain::GatewayReplicaRecoveryState::Observed
                })
        }) {
            continue;
        }
        let scope =
            postgres_gateway_scopes::find(executor, organization_id, rollback.gateway_scope_id)
                .await?;
        let target = GatewayRolloutRollbackTarget {
            scope,
            failed_rollout,
            rollback,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    Ok(targets)
}

fn rollback_recovery_state() -> TypedExpression<String> {
    sql_function::<String>(
        "jsonb_extract_path_text",
        [
            GatewayRolloutReplicas::recovery().expression(),
            bound::<String>("state").expression(),
        ],
    )
}

pub(super) async fn lock_by_id(
    transaction: &a3s_orm::PostgresTransaction,
    rollout_id: GatewayRolloutId,
) -> Result<Option<(Uuid, GatewayRollout)>, PostgresPersistenceError> {
    let row = fetch_optional::<RolloutRow, _>(
        transaction,
        select_from::<GatewayRollouts>()
            .select(RolloutSelection)
            .filter(GatewayRollouts::id().eq(rollout_id.as_uuid()))
            .for_update(),
    )
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let organization_id = row.organization_id;
    let replicas = fetch_all::<ReplicaRow, _>(
        transaction,
        select_from::<GatewayRolloutReplicas>()
            .select(ReplicaSelection)
            .filter(GatewayRolloutReplicas::gateway_rollout_id().eq(rollout_id.as_uuid()))
            .order_by(GatewayRolloutReplicas::node_id(), OrderDirection::Asc)
            .for_update(),
    )
    .await?
    .into_iter()
    .map(ReplicaRow::replica)
    .collect::<Result<Vec<_>, _>>()?;
    Ok(Some((organization_id, row.rollout(replicas)?)))
}

pub(super) async fn lock_rollback(
    transaction: &a3s_orm::PostgresTransaction,
    failed_rollout_id: GatewayRolloutId,
) -> Result<Option<GatewayRolloutRollback>, PostgresPersistenceError> {
    fetch_optional::<RollbackRow, _>(
        transaction,
        select_from::<GatewayRolloutRollbacks>()
            .select(RollbackSelection)
            .filter(GatewayRolloutRollbacks::failed_rollout_id().eq(failed_rollout_id.as_uuid()))
            .for_update(),
    )
    .await?
    .map(RollbackRow::rollback)
    .transpose()
    .map_err(Into::into)
}

pub(super) fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
