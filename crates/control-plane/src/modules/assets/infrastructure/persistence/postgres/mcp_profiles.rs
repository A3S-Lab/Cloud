use super::queries::{lock_asset, lock_release};
use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::assets::domain::{
    AssetKind, AssetReleaseArtifactKind, AssetReleaseState, McpServiceProfile,
    McpServiceProfileBinding,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_PROFILE: &str = "select organization_id, asset_id, asset_release_id, profile_digest, acl, created_at from mcp_service_profiles";

pub(super) async fn bind(
    executor: &PostgresExecutor,
    binding: McpServiceProfileBinding,
) -> Result<McpServiceProfileBinding, RepositoryError> {
    binding.validate().map_err(|error| {
        RepositoryError::Conflict(format!("invalid MCP profile write: {error}"))
    })?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let asset = lock_asset(transaction, binding.organization_id, binding.asset_id).await?;
                let release = lock_release(
                    transaction,
                    binding.organization_id,
                    binding.asset_id,
                    binding.asset_release_id,
                )
                .await?;
                if asset.kind != AssetKind::Mcp
                    || release.state != AssetReleaseState::Published
                    || release
                        .artifact
                        .as_ref()
                        .is_none_or(|artifact| artifact.kind() != AssetReleaseArtifactKind::OciService)
                {
                    return Err(RepositoryError::Conflict(
                        "MCP Service profiles require a published MCP OCI Service release".into(),
                    )
                    .into());
                }
                if binding.created_at < release.updated_at {
                    return Err(RepositoryError::Conflict(
                        "MCP Service profile binding time precedes release publication".into(),
                    )
                    .into());
                }
                if let Some(existing) = find_in_transaction(
                    transaction,
                    binding.organization_id,
                    binding.asset_id,
                    binding.asset_release_id,
                )
                .await?
                {
                    if existing == binding {
                        return Ok(existing);
                    }
                    return Err(RepositoryError::Conflict(
                        "published MCP Service profile binding is immutable".into(),
                    )
                    .into());
                }

                let result = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into mcp_service_profiles (organization_id, asset_id, asset_release_id, profile_digest, acl, created_at) values (",
                    )
                    .bind(binding.organization_id.as_uuid())
                    .append(", ")
                    .bind(binding.asset_id.as_uuid())
                    .append(", ")
                    .bind(binding.asset_release_id.as_uuid())
                    .append(", ")
                    .bind(binding.profile.digest().as_str())
                    .append(", ")
                    .bind(binding.profile.canonical_acl())
                    .append(", ")
                    .bind(binding.created_at)
                    .append(")"),
                )
                .await;
                match result {
                    Ok(rows) => require_one_row("MCP Service profile binding", rows)?,
                    Err(error) if is_unique_violation(&error) => {
                        return Err(RepositoryError::Conflict(
                            "published MCP Service profile binding is immutable".into(),
                        )
                        .into())
                    }
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(RepositoryError::NotFound.into())
                    }
                    Err(error) => return Err(error),
                }
                Ok(binding)
            })
        })
        .await
        .map_err(transaction_error)
}

pub(super) async fn find(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> Result<Option<McpServiceProfileBinding>, RepositoryError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_optional_as(profile_query(organization_id, asset_id, asset_release_id))
        .await
        .map_err(storage)?
        .map(McpServiceProfileRow::binding)
        .transpose()
}

async fn find_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> Result<Option<McpServiceProfileBinding>, PostgresPersistenceError> {
    fetch_optional::<McpServiceProfileRow, _>(
        transaction,
        profile_query(organization_id, asset_id, asset_release_id),
    )
    .await?
    .map(McpServiceProfileRow::binding)
    .transpose()
    .map_err(Into::into)
}

fn profile_query(
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
) -> a3s_orm::SqlQuery<McpServiceProfileRow> {
    sql_query::<McpServiceProfileRow>(SELECT_PROFILE)
        .append(" where organization_id = ")
        .bind(organization_id.as_uuid())
        .append(" and asset_id = ")
        .bind(asset_id.as_uuid())
        .append(" and asset_release_id = ")
        .bind(asset_release_id.as_uuid())
}

struct McpServiceProfileRow {
    organization_id: Uuid,
    asset_id: Uuid,
    asset_release_id: Uuid,
    profile_digest: String,
    acl: String,
    created_at: DateTime<Utc>,
}

impl FromRow for McpServiceProfileRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            asset_id: decode(row, 1)?,
            asset_release_id: decode(row, 2)?,
            profile_digest: decode(row, 3)?,
            acl: decode(row, 4)?,
            created_at: decode(row, 5)?,
        })
    }
}

impl McpServiceProfileRow {
    fn binding(self) -> Result<McpServiceProfileBinding, RepositoryError> {
        let binding = McpServiceProfileBinding {
            organization_id: OrganizationId::from_uuid(self.organization_id),
            asset_id: AssetId::from_uuid(self.asset_id),
            asset_release_id: AssetReleaseId::from_uuid(self.asset_release_id),
            profile: McpServiceProfile::restore(&self.acl, &self.profile_digest).map_err(stored)?,
            created_at: self.created_at,
        };
        binding.validate().map_err(stored)?;
        Ok(binding)
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("stored MCP Service profile is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}
