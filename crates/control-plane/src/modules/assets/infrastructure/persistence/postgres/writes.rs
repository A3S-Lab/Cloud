use super::queries::{load_asset_write, load_release_write, lock_asset, lock_release};
use crate::infrastructure::{
    execute, idempotency_replay, is_foreign_key_violation, is_unique_violation, require_one_row,
    store_idempotency, store_outbox, transaction_error, PostgresPersistenceError,
};
use crate::modules::assets::domain::{
    AssetReleaseWrite, AssetReleaseWriteReference, AssetState, AssetWrite, AssetWriteReference,
    CreateAssetReleaseWrite, CreateAssetWrite, TransitionAssetReleaseWrite, TransitionAssetWrite,
};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OrganizationId, RepositoryError};
use a3s_orm::{sql_query, PostgresExecutor, PostgresTransaction};

pub(super) async fn create_asset(
    executor: &PostgresExecutor,
    bundle: CreateAssetWrite,
) -> Result<AssetWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_asset(
                    transaction,
                    bundle.asset.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into assets (organization_id, id, name, name_key, kind, state, aggregate_version, created_at, updated_at, archived_at) values (",
                    )
                    .bind(bundle.asset.organization_id.as_uuid())
                    .append(", ")
                    .bind(bundle.asset.id.as_uuid())
                    .append(", ")
                    .bind(bundle.asset.name.as_str())
                    .append(", ")
                    .bind(bundle.asset.name.key())
                    .append(", ")
                    .bind(bundle.asset.kind.as_str())
                    .append(", ")
                    .bind(bundle.asset.state.as_str())
                    .append(", ")
                    .bind(bundle.asset.aggregate_version)
                    .append(", ")
                    .bind(bundle.asset.created_at)
                    .append(", ")
                    .bind(bundle.asset.updated_at)
                    .append(", ")
                    .bind(bundle.asset.archived_at)
                    .append(")"),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("Asset", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "Asset identity or organization-scoped name is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                store_outbox(transaction, &bundle.event).await?;
                let reference = AssetWriteReference {
                    asset_id: bundle.asset.id,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(AssetWrite {
                    asset: bundle.asset,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn transition_asset(
    executor: &PostgresExecutor,
    bundle: TransitionAssetWrite,
) -> Result<AssetWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_asset(
                    transaction,
                    bundle.asset.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let existing =
                    lock_asset(transaction, bundle.asset.organization_id, bundle.asset.id).await?;
                bundle
                    .validate_against(&existing)
                    .map_err(invalid_transition)?;
                require_one_row(
                    "Asset transition",
                    execute(
                        transaction,
                        sql_query::<()>("update assets set state = ")
                            .bind(bundle.asset.state.as_str())
                            .append(", aggregate_version = ")
                            .bind(bundle.asset.aggregate_version)
                            .append(", updated_at = ")
                            .bind(bundle.asset.updated_at)
                            .append(", archived_at = ")
                            .bind(bundle.asset.archived_at)
                            .append(" where organization_id = ")
                            .bind(bundle.asset.organization_id.as_uuid())
                            .append(" and id = ")
                            .bind(bundle.asset.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_aggregate_version),
                    )
                    .await?,
                )?;
                store_outbox(transaction, &bundle.event).await?;
                let reference = AssetWriteReference {
                    asset_id: bundle.asset.id,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(AssetWrite {
                    asset: bundle.asset,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn create_release(
    executor: &PostgresExecutor,
    bundle: CreateAssetReleaseWrite,
) -> Result<AssetReleaseWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_release(
                    transaction,
                    bundle.release.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let asset = lock_asset(
                    transaction,
                    bundle.release.organization_id,
                    bundle.release.asset_id,
                )
                .await?;
                bundle
                    .release
                    .validate_for(&asset)
                    .map_err(invalid_transition)?;
                if asset.state != AssetState::Active {
                    return Err(RepositoryError::Conflict(
                        "archived Asset cannot create a release".into(),
                    )
                    .into());
                }
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into asset_releases (organization_id, asset_id, id, version, state, commit_sha, manifest_digest, artifact_kind, artifact_digest, artifact_media_type, artifact_size_bytes, aggregate_version, created_at, updated_at, published_at, yanked_at) values (",
                    )
                    .bind(bundle.release.organization_id.as_uuid())
                    .append(", ")
                    .bind(bundle.release.asset_id.as_uuid())
                    .append(", ")
                    .bind(bundle.release.id.as_uuid())
                    .append(", ")
                    .bind(bundle.release.version.as_str())
                    .append(", ")
                    .bind(bundle.release.state.as_str())
                    .append(", ")
                    .bind(bundle.release.commit_sha.as_str())
                    .append(", ")
                    .bind(bundle.release.manifest_digest.as_str())
                    .append(", null, null, null, null, ")
                    .bind(bundle.release.aggregate_version)
                    .append(", ")
                    .bind(bundle.release.created_at)
                    .append(", ")
                    .bind(bundle.release.updated_at)
                    .append(", null, null)"),
                )
                .await;
                match inserted {
                    Ok(rows) => require_one_row("Asset release", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "Asset release identity or version is already in use".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                store_outbox(transaction, &bundle.event).await?;
                let reference = AssetReleaseWriteReference {
                    asset_id: bundle.release.asset_id,
                    asset_release_id: bundle.release.id,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(AssetReleaseWrite {
                    asset,
                    release: bundle.release,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn transition_release(
    executor: &PostgresExecutor,
    bundle: TransitionAssetReleaseWrite,
) -> Result<AssetReleaseWrite, RepositoryError> {
    bundle.validate().map_err(invalid_repository_write)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) = replay_release(
                    transaction,
                    bundle.release.organization_id,
                    &bundle.idempotency,
                )
                .await?
                {
                    return Ok(replay);
                }
                let asset = lock_asset(
                    transaction,
                    bundle.release.organization_id,
                    bundle.release.asset_id,
                )
                .await?;
                let existing = lock_release(
                    transaction,
                    bundle.release.organization_id,
                    bundle.release.asset_id,
                    bundle.release.id,
                )
                .await?;
                bundle
                    .validate_against(&existing, &asset)
                    .map_err(invalid_transition)?;
                let (artifact_kind, artifact_digest, artifact_media_type, artifact_size_bytes) =
                    bundle
                        .release
                        .artifact
                        .as_ref()
                        .map_or((None, None, None, None), |artifact| {
                            (
                                Some(artifact.kind().as_str()),
                                Some(artifact.digest().as_str()),
                                Some(artifact.media_type()),
                                Some(artifact.size_bytes()),
                            )
                        });
                require_one_row(
                    "Asset release transition",
                    execute(
                        transaction,
                        sql_query::<()>("update asset_releases set state = ")
                            .bind(bundle.release.state.as_str())
                            .append(", artifact_kind = ")
                            .bind(artifact_kind)
                            .append(", artifact_digest = ")
                            .bind(artifact_digest)
                            .append(", artifact_media_type = ")
                            .bind(artifact_media_type)
                            .append(", artifact_size_bytes = ")
                            .bind(artifact_size_bytes)
                            .append(", aggregate_version = ")
                            .bind(bundle.release.aggregate_version)
                            .append(", updated_at = ")
                            .bind(bundle.release.updated_at)
                            .append(", published_at = ")
                            .bind(bundle.release.published_at)
                            .append(", yanked_at = ")
                            .bind(bundle.release.yanked_at)
                            .append(" where organization_id = ")
                            .bind(bundle.release.organization_id.as_uuid())
                            .append(" and asset_id = ")
                            .bind(bundle.release.asset_id.as_uuid())
                            .append(" and id = ")
                            .bind(bundle.release.id.as_uuid())
                            .append(" and aggregate_version = ")
                            .bind(bundle.expected_aggregate_version),
                    )
                    .await?,
                )?;
                store_outbox(transaction, &bundle.event).await?;
                let reference = AssetReleaseWriteReference {
                    asset_id: bundle.release.asset_id,
                    asset_release_id: bundle.release.id,
                };
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(AssetReleaseWrite {
                    asset,
                    release: bundle.release,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn replay_asset(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<AssetWrite>, PostgresPersistenceError> {
    let Some(replay) = idempotency_replay::<AssetWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    load_asset_write(transaction, organization_id, replay.value, true)
        .await
        .map(Some)
}

async fn replay_release(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    idempotency: &IdempotencyRequest,
) -> Result<Option<AssetReleaseWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<AssetReleaseWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    load_release_write(transaction, organization_id, replay.value, true)
        .await
        .map(Some)
}

fn invalid_repository_write(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!("invalid Asset repository write: {error}"))
}

fn invalid_transition(error: String) -> PostgresPersistenceError {
    RepositoryError::Conflict(error).into()
}
