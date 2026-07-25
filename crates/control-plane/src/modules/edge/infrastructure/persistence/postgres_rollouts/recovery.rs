use super::{lock_by_id, persist_recovery_transition};
use crate::infrastructure::{fetch_optional, transaction_error, PostgresPersistenceError};
use crate::modules::edge::domain::repositories::GatewayReplicaRecoveryTarget;
use crate::modules::edge::domain::{
    GatewayPublication, GatewayReplicaRecoveryState, GatewayRollout,
};
use crate::modules::edge::infrastructure::persistence::postgres::{
    PublicationRow, PublicationSelection,
};
use crate::modules::edge::infrastructure::persistence::postgres_schema::{
    GatewayPublications, GatewayRolloutReplicas, GatewayRollouts,
};
use crate::modules::shared_kernel::domain::{
    GatewayRolloutId, NodeCommandId, NodeId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::NodeGatewaySnapshotObservation;
use a3s_orm::function::{bound, sql_function, TypedExpression};
use a3s_orm::{select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(in crate::modules::edge::infrastructure::persistence) async fn pending(
    executor: &PostgresExecutor,
    limit: usize,
) -> Result<Vec<GatewayReplicaRecoveryTarget>, RepositoryError> {
    validate_batch_limit(limit)?;
    let limit = u64::try_from(limit).map_err(|_| {
        RepositoryError::Conflict("Gateway replica recovery limit exceeds supported range".into())
    })?;
    let identities = Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            select_from::<GatewayRollouts>()
                .inner_join::<GatewayRolloutReplicas>(
                    GatewayRollouts::id().eq_column(GatewayRolloutReplicas::gateway_rollout_id()),
                )
                .select((
                    GatewayRollouts::organization_id(),
                    GatewayRollouts::id(),
                    GatewayRolloutReplicas::node_id(),
                ))
                .filter(GatewayRolloutReplicas::state().eq("unavailable"))
                .filter(
                    recovery_state()
                        .eq("required")
                        .or(recovery_state().eq("observing")),
                )
                .order_by(GatewayRollouts::started_at(), OrderDirection::Asc)
                .order_by(GatewayRollouts::id(), OrderDirection::Asc)
                .order_by(GatewayRolloutReplicas::node_id(), OrderDirection::Asc)
                .limit(limit),
        )
        .await
        .map_err(storage)?
        .rows;

    let mut targets = Vec::with_capacity(identities.len());
    for (organization_id, rollout_id, node_id) in identities {
        let organization_id = OrganizationId::from_uuid(organization_id);
        let rollout_id = GatewayRolloutId::from_uuid(rollout_id);
        let node_id = NodeId::from_uuid(node_id);
        let rollout = super::find(executor, organization_id, rollout_id).await?;
        let publication = publication(executor, node_id, rollout_id, &rollout).await?;
        let prior_publication = match publication.expected_revision {
            Some(revision) => Some(
                fetch_publication(executor, node_id, revision)
                    .await?
                    .ok_or_else(|| {
                        RepositoryError::Storage(
                            "Gateway replica recovery prior publication disappeared".into(),
                        )
                    })?,
            ),
            None => None,
        };
        let target = GatewayReplicaRecoveryTarget {
            organization_id,
            rollout,
            publication,
            prior_publication,
        };
        target.validate().map_err(RepositoryError::Storage)?;
        targets.push(target);
    }
    Ok(targets)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::modules::edge::infrastructure::persistence) async fn stage_observation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    command_id: NodeCommandId,
    issued_at: DateTime<Utc>,
    not_after: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let (_, mut rollout, _, _) = lock_context(
                    transaction,
                    organization_id,
                    rollout_id,
                    node_id,
                    expected_version,
                )
                .await?;
                rollout
                    .stage_recovery_observation(node_id, command_id, issued_at, not_after)
                    .map_err(RepositoryError::Conflict)?;
                persist_recovery_transition(transaction, &rollout, node_id, expected_version)
                    .await?;
                Ok(rollout)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(in crate::modules::edge::infrastructure::persistence) async fn record_observation(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    observation: NodeGatewaySnapshotObservation,
) -> Result<GatewayRollout, RepositoryError> {
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let (_, mut rollout, candidate, prior) = lock_context(
                    transaction,
                    organization_id,
                    rollout_id,
                    node_id,
                    expected_version,
                )
                .await?;
                let changed = rollout
                    .record_recovery_observation(
                        node_id,
                        &candidate,
                        prior.as_ref(),
                        observation,
                    )
                    .map_err(RepositoryError::Conflict)?;
                if changed {
                    let recovery = rollout
                        .replicas
                        .iter()
                        .find(|replica| replica.node_id == node_id)
                        .and_then(|replica| replica.recovery.as_ref())
                        .cloned()
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "Gateway replica recovery disappeared after observation".into(),
                            )
                        })?;
                    match recovery.state {
                        GatewayReplicaRecoveryState::Observed => {
                            let installed_revision = recovery
                                .observation
                                .as_ref()
                                .and_then(|observation| observation.applied.as_ref())
                                .map(|applied| applied.revision);
                            super::persist_recovered_physical_scope(
                                transaction,
                                node_id,
                                installed_revision,
                                recovery.updated_at,
                            )
                            .await?;
                        }
                        GatewayReplicaRecoveryState::Diverged => {
                            super::diverge_required_rollback(
                                transaction,
                                rollout.id,
                                recovery.failure.as_deref().unwrap_or(
                                    "Gateway physical state diverged during exact rollback recovery",
                                ),
                                recovery.updated_at,
                            )
                            .await?;
                        }
                        GatewayReplicaRecoveryState::Required
                        | GatewayReplicaRecoveryState::Observing => {}
                    }
                }
                persist_recovery_transition(transaction, &rollout, node_id, expected_version)
                    .await?;
                Ok(rollout)
            })
        })
        .await
        .map_err(transaction_error)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::modules::edge::infrastructure::persistence) async fn record_failure(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
    command_id: NodeCommandId,
    failure: &str,
    retryable: bool,
    failed_at: DateTime<Utc>,
) -> Result<GatewayRollout, RepositoryError> {
    let failure = failure.to_owned();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let (_, mut rollout, _, _) = lock_context(
                    transaction,
                    organization_id,
                    rollout_id,
                    node_id,
                    expected_version,
                )
                .await?;
                rollout
                    .record_recovery_command_failure(
                        node_id, command_id, failure, retryable, failed_at,
                    )
                    .map_err(RepositoryError::Conflict)?;
                persist_recovery_transition(transaction, &rollout, node_id, expected_version)
                    .await?;
                Ok(rollout)
            })
        })
        .await
        .map_err(transaction_error)
}

async fn lock_context(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    rollout_id: GatewayRolloutId,
    node_id: NodeId,
    expected_version: u64,
) -> Result<
    (
        Uuid,
        GatewayRollout,
        GatewayPublication,
        Option<GatewayPublication>,
    ),
    PostgresPersistenceError,
> {
    let (stored_organization_id, rollout) = lock_by_id(transaction, rollout_id)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    if stored_organization_id != organization_id.as_uuid() {
        return Err(RepositoryError::NotFound.into());
    }
    if rollout.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(
            "Gateway rollout changed before its recovery transition".into(),
        )
        .into());
    }
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            RepositoryError::Conflict("Gateway rollout does not contain this member".into())
        })?;
    let candidate = fetch_optional::<PublicationRow, _>(
        transaction,
        select_from::<GatewayPublications>()
            .select(PublicationSelection)
            .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
            .filter(GatewayPublications::revision().eq(replica.revision))
            .filter(GatewayPublications::command_id().eq(replica.command_id.as_uuid()))
            .for_update(),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "Gateway replica recovery candidate publication disappeared".into(),
        )
    })?
    .publication()?;
    let prior = match candidate.expected_revision {
        Some(revision) => Some(
            fetch_optional::<PublicationRow, _>(
                transaction,
                select_from::<GatewayPublications>()
                    .select(PublicationSelection)
                    .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                    .filter(GatewayPublications::revision().eq(revision))
                    .for_update(),
            )
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "Gateway replica recovery prior publication disappeared".into(),
                )
            })?
            .publication()?,
        ),
        None => None,
    };
    Ok((stored_organization_id, rollout, candidate, prior))
}

async fn publication(
    executor: &PostgresExecutor,
    node_id: NodeId,
    rollout_id: GatewayRolloutId,
    rollout: &GatewayRollout,
) -> Result<GatewayPublication, RepositoryError> {
    let replica = rollout
        .replicas
        .iter()
        .find(|replica| replica.node_id == node_id)
        .ok_or_else(|| {
            RepositoryError::Storage("Gateway recovery target rollout omitted its replica".into())
        })?;
    let publication = fetch_publication(executor, node_id, replica.revision)
        .await?
        .ok_or_else(|| {
            RepositoryError::Storage(
                "Gateway replica recovery candidate publication disappeared".into(),
            )
        })?;
    if publication.command_id != replica.command_id {
        return Err(RepositoryError::Storage(format!(
            "Gateway rollout {rollout_id} recovery publication command is inconsistent"
        )));
    }
    Ok(publication)
}

async fn fetch_publication(
    executor: &PostgresExecutor,
    node_id: NodeId,
    revision: u64,
) -> Result<Option<GatewayPublication>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(
            select_from::<GatewayPublications>()
                .select(PublicationSelection)
                .filter(GatewayPublications::node_id().eq(node_id.as_uuid()))
                .filter(GatewayPublications::revision().eq(revision)),
        )
        .await
        .map_err(storage)?
        .map(PublicationRow::publication)
        .transpose()
}

fn recovery_state() -> TypedExpression<String> {
    sql_function::<String>(
        "jsonb_extract_path_text",
        [
            GatewayRolloutReplicas::recovery().expression(),
            bound::<String>("state").expression(),
        ],
    )
}

fn validate_batch_limit(limit: usize) -> Result<(), RepositoryError> {
    if limit == 0 || limit > 10_000 {
        return Err(RepositoryError::Conflict(
            "Gateway replica recovery batch limit is invalid".into(),
        ));
    }
    Ok(())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
