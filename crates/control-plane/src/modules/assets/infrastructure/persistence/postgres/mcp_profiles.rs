use super::queries::{lock_asset, lock_release};
use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::assets::domain::{
    AssetKind, AssetReleaseArtifactKind, AssetReleaseState, BindMcpServiceProfileWrite,
    McpServiceProfile, McpServiceProfileBinding, McpServiceProfileWrite,
    McpServiceProfileWriteReference,
};
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, IdempotencyRequest, OrganizationId, RepositoryError, Sha256Digest,
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
    bundle: BindMcpServiceProfileWrite,
) -> Result<McpServiceProfileWrite, RepositoryError> {
    bundle.validate().map_err(|error| {
        RepositoryError::Conflict(format!("invalid MCP profile write: {error}"))
    })?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                if let Some(replay) =
                    replay(transaction, &bundle.binding, &bundle.idempotency).await?
                {
                    return Ok(replay);
                }
                let binding = bundle.binding;
                let asset =
                    lock_asset(transaction, binding.organization_id, binding.asset_id).await?;
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
                    if existing.profile == binding.profile {
                        store_profile_audit(
                            transaction,
                            &binding,
                            bundle.event.correlation_id,
                            false,
                        )
                        .await?;
                        let reference = reference(&existing);
                        store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                        return Ok(McpServiceProfileWrite {
                            binding: existing,
                            replayed: true,
                        });
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
                store_outbox(transaction, &bundle.event).await?;
                store_profile_audit(
                    transaction,
                    &binding,
                    bundle.event.correlation_id,
                    true,
                )
                .await?;
                let reference = reference(&binding);
                store_idempotency(transaction, &bundle.idempotency, &reference).await?;
                Ok(McpServiceProfileWrite {
                    binding,
                    replayed: false,
                })
            })
        })
        .await
        .map_err(transaction_error)
}

async fn replay(
    transaction: &PostgresTransaction,
    expected: &McpServiceProfileBinding,
    idempotency: &IdempotencyRequest,
) -> Result<Option<McpServiceProfileWrite>, PostgresPersistenceError> {
    let Some(replay) =
        idempotency_replay::<McpServiceProfileWriteReference>(transaction, idempotency).await?
    else {
        return Ok(None);
    };
    if replay.value.asset_id.as_uuid().is_nil()
        || replay.value.asset_release_id.as_uuid().is_nil()
        || Sha256Digest::parse(&replay.value.profile_digest).is_err()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored MCP Service profile idempotency reference is invalid".into(),
        ));
    }
    if replay.value.asset_id != expected.asset_id
        || replay.value.asset_release_id != expected.asset_release_id
        || replay.value.profile_digest != expected.profile.digest().as_str()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored MCP Service profile idempotency reference does not match the request".into(),
        ));
    }
    let binding = find_in_transaction(
        transaction,
        expected.organization_id,
        replay.value.asset_id,
        replay.value.asset_release_id,
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "MCP Service profile idempotency target is missing".into(),
        )
    })?;
    if binding.profile.digest().as_str() != replay.value.profile_digest {
        return Err(PostgresPersistenceError::Invariant(
            "MCP Service profile idempotency target changed".into(),
        ));
    }
    Ok(Some(McpServiceProfileWrite {
        binding,
        replayed: true,
    }))
}

fn reference(binding: &McpServiceProfileBinding) -> McpServiceProfileWriteReference {
    McpServiceProfileWriteReference {
        asset_id: binding.asset_id,
        asset_release_id: binding.asset_release_id,
        profile_digest: binding.profile.digest().to_string(),
    }
}

async fn store_profile_audit(
    transaction: &PostgresTransaction,
    binding: &McpServiceProfileBinding,
    request_id: Uuid,
    changed: bool,
) -> Result<(), PostgresPersistenceError> {
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            scope: AuditWrite::organization_scope(binding.organization_id.as_uuid()),
            actor_id: None,
            action: "asset.mcp-service-profile.bound",
            aggregate_id: binding.asset_release_id.as_uuid(),
            occurred_at: binding.created_at,
            request_id,
            details: serde_json::json!({
                "assetId": binding.asset_id,
                "assetReleaseId": binding.asset_release_id,
                "profileDigest": binding.profile.digest().as_str(),
                "changed": changed,
            }),
        },
    )
    .await
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
