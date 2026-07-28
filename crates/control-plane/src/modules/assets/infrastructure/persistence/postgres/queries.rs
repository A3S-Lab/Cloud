use super::rows::{AssetReleaseRow, AssetRow, SELECT_ASSETS, SELECT_RELEASES};
use crate::infrastructure::{fetch_optional, PostgresPersistenceError};
use crate::modules::assets::domain::{
    Asset, AssetRelease, AssetReleaseWrite, AssetReleaseWriteReference, AssetWrite,
    AssetWriteReference,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor, PostgresTransaction};

pub(super) async fn find_asset(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    asset_id: AssetId,
) -> Result<Option<Asset>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(asset_query(organization_id, asset_id))
        .await
        .map_err(storage)?
        .map(AssetRow::asset)
        .transpose()
}

pub(super) async fn list_assets(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
) -> Result<Vec<Asset>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<AssetRow>(SELECT_ASSETS)
                .append(" where a.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" order by a.created_at, a.id"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(AssetRow::asset)
        .collect()
}

pub(super) async fn find_release(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> Result<Option<AssetRelease>, RepositoryError> {
    let Some(row) = Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(release_query(organization_id, asset_id, asset_release_id))
        .await
        .map_err(storage)?
    else {
        return Ok(None);
    };
    let asset = find_asset(executor, organization_id, asset_id)
        .await?
        .ok_or_else(|| RepositoryError::Storage("stored Asset release lost its Asset".into()))?;
    let release = row.release()?;
    release.validate_for(&asset).map_err(stored_release)?;
    Ok(Some(release))
}

pub(super) async fn list_releases(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    asset_id: AssetId,
) -> Result<Vec<AssetRelease>, RepositoryError> {
    let Some(asset) = find_asset(executor, organization_id, asset_id).await? else {
        return Ok(Vec::new());
    };
    Database::new(PostgresDialect, executor.clone())
        .fetch_all_as(
            sql_query::<AssetReleaseRow>(SELECT_RELEASES)
                .append(" where r.organization_id = ")
                .bind(organization_id.as_uuid())
                .append(" and r.asset_id = ")
                .bind(asset_id.as_uuid())
                .append(" order by r.created_at, r.id"),
        )
        .await
        .map_err(storage)?
        .rows
        .into_iter()
        .map(AssetReleaseRow::release)
        .map(|release| {
            let release = release?;
            release.validate_for(&asset).map_err(stored_release)?;
            Ok(release)
        })
        .collect()
}

pub(super) async fn lock_asset(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    asset_id: AssetId,
) -> Result<Asset, PostgresPersistenceError> {
    fetch_optional::<AssetRow, _>(
        transaction,
        asset_query(organization_id, asset_id).append(" for update"),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .asset()
    .map_err(Into::into)
}

pub(super) async fn lock_release(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> Result<AssetRelease, PostgresPersistenceError> {
    fetch_optional::<AssetReleaseRow, _>(
        transaction,
        release_query(organization_id, asset_id, asset_release_id).append(" for update"),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .release()
    .map_err(Into::into)
}

pub(super) async fn load_asset_write(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    reference: AssetWriteReference,
    replayed: bool,
) -> Result<AssetWrite, PostgresPersistenceError> {
    let asset = fetch_optional::<AssetRow, _>(
        transaction,
        asset_query(organization_id, reference.asset_id),
    )
    .await?
    .ok_or_else(|| invalid_reference("Asset"))?
    .asset()?;
    Ok(AssetWrite { asset, replayed })
}

pub(super) async fn load_release_write(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    reference: AssetReleaseWriteReference,
    replayed: bool,
) -> Result<AssetReleaseWrite, PostgresPersistenceError> {
    let asset = fetch_optional::<AssetRow, _>(
        transaction,
        asset_query(organization_id, reference.asset_id),
    )
    .await?
    .ok_or_else(|| invalid_reference("Asset"))?
    .asset()?;
    let release = fetch_optional::<AssetReleaseRow, _>(
        transaction,
        release_query(
            organization_id,
            reference.asset_id,
            reference.asset_release_id,
        ),
    )
    .await?
    .ok_or_else(|| invalid_reference("Asset release"))?
    .release()?;
    release.validate_for(&asset).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "Asset release idempotency reference is invalid: {error}"
        ))
    })?;
    Ok(AssetReleaseWrite {
        asset,
        release,
        replayed,
    })
}

fn asset_query(organization_id: OrganizationId, asset_id: AssetId) -> a3s_orm::SqlQuery<AssetRow> {
    sql_query::<AssetRow>(SELECT_ASSETS)
        .append(" where a.organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and a.id = ")
        .bind(asset_id.as_uuid())
}

fn release_query(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> a3s_orm::SqlQuery<AssetReleaseRow> {
    sql_query::<AssetReleaseRow>(SELECT_RELEASES)
        .append(" where r.organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and r.asset_id = ")
        .bind(asset_id.as_uuid())
        .append(" and r.id = ")
        .bind(asset_release_id.as_uuid())
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn stored_release(error: String) -> RepositoryError {
    RepositoryError::Storage(format!(
        "stored Asset release does not match its Asset: {error}"
    ))
}

fn invalid_reference(resource: &str) -> PostgresPersistenceError {
    PostgresPersistenceError::Invariant(format!("{resource} idempotency reference is invalid"))
}
