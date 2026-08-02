use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, require_one_row, store_audit, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::assets::domain::{
    AcquireAssetGitWriteLease, AssetGitRepositoryControlError, AssetGitWriteJournal,
    AssetGitWriteLease, AssetGitWriteOperation, AssetGitWriteRecovery, ClaimAssetGitWriteRecovery,
    CompleteAssetGitWriteLease,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction,
    PostgresTransactionError, Row,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

const SELECT_CONTROL: &str = "select organization_id, asset_id, quota_bytes, observed_bytes, write_lease_id, write_lease_operation, write_lease_actor_id, write_lease_request_id, write_leased_until, write_lease_recovering, write_cleanup_lease_id from asset_git_repository_controls";

pub(super) async fn claim_write_recovery(
    executor: &PostgresExecutor,
    request: ClaimAssetGitWriteRecovery,
) -> Result<Option<AssetGitWriteRecovery>, AssetGitRepositoryControlError> {
    request
        .validate()
        .map_err(AssetGitRepositoryControlError::Invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let Some(row) = lock_control(
                    transaction,
                    request.asset.organization_id.as_uuid(),
                    request.asset.id.as_uuid(),
                )
                .await?
                else {
                    return Ok(None);
                };
                row.validate()?;
                if let Some(lease_id) = row.write_cleanup_lease_id {
                    return Ok(Some(AssetGitWriteRecovery::Cleanup(AssetGitWriteJournal {
                        organization_id: request.asset.organization_id,
                        asset_id: request.asset.id,
                        lease_id,
                    })));
                }
                let Some(leased_until) = row.write_leased_until else {
                    return Ok(None);
                };
                if leased_until > request.claimed_at {
                    return Ok(Some(AssetGitWriteRecovery::Active));
                }
                let lease = row.to_lease(request.leased_until, true)?;
                require_one_row(
                    "Asset Git write recovery claim",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update asset_git_repository_controls set write_leased_until = ",
                        )
                        .bind(request.leased_until)
                        .append(", write_lease_recovering = true, updated_at = ")
                        .bind(request.claimed_at)
                        .append(" where organization_id = ")
                        .bind(request.asset.organization_id.as_uuid())
                        .append(" and asset_id = ")
                        .bind(request.asset.id.as_uuid())
                        .append(" and write_lease_id = ")
                        .bind(lease.lease_id),
                    )
                    .await?,
                )?;
                Ok(Some(AssetGitWriteRecovery::Rollback(lease)))
            })
        })
        .await
        .map_err(map_transaction)
}

pub(super) async fn acquire_write(
    executor: &PostgresExecutor,
    request: AcquireAssetGitWriteLease,
) -> Result<AssetGitWriteLease, AssetGitRepositoryControlError> {
    request
        .validate()
        .map_err(AssetGitRepositoryControlError::Invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let inserted = execute(
                    transaction,
                    sql_query::<()>(
                        "insert into asset_git_repository_controls (organization_id, asset_id, quota_bytes, observed_bytes, updated_at) values (",
                    )
                    .bind(request.asset.organization_id.as_uuid())
                    .append(", ")
                    .bind(request.asset.id.as_uuid())
                    .append(", ")
                    .bind(request.default_quota_bytes)
                    .append(", ")
                    .bind(request.observed_bytes)
                    .append(", ")
                    .bind(request.acquired_at)
                    .append(") on conflict (organization_id, asset_id) do nothing"),
                )
                .await;
                match inserted {
                    Ok(_) => {}
                    Err(error) if is_foreign_key_violation(&error) => {
                        return Err(AssetGitRepositoryControlError::NotFound.into())
                    }
                    Err(error) => return Err(error.into()),
                }
                let row = lock_control(
                    transaction,
                    request.asset.organization_id.as_uuid(),
                    request.asset.id.as_uuid(),
                )
                .await?
                .ok_or(AssetGitRepositoryControlError::NotFound)?;
                row.validate()?;
                if row.write_cleanup_lease_id.is_some() || row.write_lease_id.is_some() {
                    if row
                        .write_leased_until
                        .is_some_and(|leased_until| leased_until > request.acquired_at)
                    {
                        return Err(AssetGitRepositoryControlError::Busy.into());
                    }
                    return Err(AssetGitRepositoryControlError::RecoveryRequired.into());
                }
                if request.observed_bytes > row.quota_bytes {
                    return Err(AssetGitRepositoryControlError::QuotaExceeded {
                        quota_bytes: row.quota_bytes,
                        observed_bytes: request.observed_bytes,
                    }
                    .into());
                }
                require_one_row(
                    "Asset Git write lease",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update asset_git_repository_controls set observed_bytes = ",
                        )
                        .bind(request.observed_bytes)
                        .append(", write_lease_id = ")
                        .bind(request.lease_id)
                        .append(", write_lease_operation = ")
                        .bind(request.operation.as_str())
                        .append(", write_lease_actor_id = ")
                        .bind(request.actor_id)
                        .append(", write_lease_request_id = ")
                        .bind(request.request_id)
                        .append(", write_leased_until = ")
                        .bind(request.leased_until)
                        .append(", write_lease_recovering = false, updated_at = ")
                        .bind(request.acquired_at)
                        .append(" where organization_id = ")
                        .bind(request.asset.organization_id.as_uuid())
                        .append(" and asset_id = ")
                        .bind(request.asset.id.as_uuid()),
                    )
                    .await?,
                )?;
                Ok(AssetGitWriteLease {
                    organization_id: request.asset.organization_id,
                    asset_id: request.asset.id,
                    lease_id: request.lease_id,
                    operation: request.operation,
                    actor_id: request.actor_id,
                    request_id: request.request_id,
                    quota_bytes: row.quota_bytes,
                    observed_bytes: request.observed_bytes,
                    leased_until: request.leased_until,
                    recovery: false,
                })
            })
        })
        .await
        .map_err(map_transaction)
}

pub(super) async fn complete_write(
    executor: &PostgresExecutor,
    completion: CompleteAssetGitWriteLease,
) -> Result<(), AssetGitRepositoryControlError> {
    if completion.observed_bytes > completion.lease.quota_bytes {
        return Err(AssetGitRepositoryControlError::QuotaExceeded {
            quota_bytes: completion.lease.quota_bytes,
            observed_bytes: completion.observed_bytes,
        });
    }
    completion
        .validate()
        .map_err(AssetGitRepositoryControlError::Invalid)?;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = lock_control(
                    transaction,
                    completion.lease.organization_id.as_uuid(),
                    completion.lease.asset_id.as_uuid(),
                )
                .await?
                .ok_or(AssetGitRepositoryControlError::NotFound)?;
                row.validate()?;
                if !row.matches(&completion.lease) {
                    return Err(AssetGitRepositoryControlError::StaleLease.into());
                }
                let mut query = sql_query::<()>(
                    "update asset_git_repository_controls set observed_bytes = ",
                )
                .bind(completion.observed_bytes)
                .append(", write_lease_id = null, write_lease_operation = null, write_lease_actor_id = null, write_lease_request_id = null, write_leased_until = null, write_lease_recovering = false, write_cleanup_lease_id = ")
                .bind(completion.lease.lease_id);
                if let Some(backup) = &completion.backup {
                    query = query
                        .append(", latest_backup_object_key = ")
                        .bind(backup.object_key.as_str())
                        .append(", latest_backup_digest = ")
                        .bind(backup.digest.as_str())
                        .append(", latest_backup_size_bytes = ")
                        .bind(backup.size_bytes)
                        .append(", latest_backup_refs_digest = ")
                        .bind(backup.refs_digest.as_str())
                        .append(", latest_backup_created_at = ")
                        .bind(backup.created_at);
                }
                query = query
                    .append(", updated_at = ")
                    .bind(completion.completed_at)
                    .append(" where organization_id = ")
                    .bind(completion.lease.organization_id.as_uuid())
                    .append(" and asset_id = ")
                    .bind(completion.lease.asset_id.as_uuid())
                    .append(" and write_lease_id = ")
                    .bind(completion.lease.lease_id);
                require_one_row(
                    "Asset Git write completion",
                    execute(transaction, query).await?,
                )?;
                let backup = completion.backup.as_ref().map(|backup| {
                    json!({
                        "digest": backup.digest.as_str(),
                        "objectKey": backup.object_key,
                        "refsDigest": backup.refs_digest.as_str(),
                        "sizeBytes": backup.size_bytes,
                    })
                });
                store_audit(
                    transaction,
                    &AuditWrite {
                        audit_id: Uuid::now_v7(),
                        organization_id: completion.lease.organization_id.as_uuid(),
                        actor_id: Some(completion.lease.actor_id),
                        action: completion.lease.operation.audit_action(),
                        aggregate_id: completion.lease.asset_id.as_uuid(),
                        occurred_at: completion.completed_at,
                        request_id: completion.lease.request_id,
                        details: json!({
                            "schema": "a3s.cloud.asset-git-audit.v1",
                            "operation": completion.lease.operation.as_str(),
                            "quotaBytes": completion.lease.quota_bytes,
                            "refsDigest": completion.refs_digest.as_str(),
                            "repositoryBytes": completion.observed_bytes,
                            "backup": backup,
                        }),
                    },
                )
                .await?;
                Ok(())
            })
        })
        .await
        .map_err(map_transaction)
}

pub(super) async fn abandon_write(
    executor: &PostgresExecutor,
    lease: &AssetGitWriteLease,
) -> Result<(), AssetGitRepositoryControlError> {
    let lease = lease.clone();
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = lock_control(
                    transaction,
                    lease.organization_id.as_uuid(),
                    lease.asset_id.as_uuid(),
                )
                .await?
                .ok_or(AssetGitRepositoryControlError::NotFound)?;
                row.validate()?;
                if !row.matches(&lease) {
                    return Err(AssetGitRepositoryControlError::StaleLease.into());
                }
                require_one_row(
                    "abandoned Asset Git write lease",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update asset_git_repository_controls set write_lease_id = null, write_lease_operation = null, write_lease_actor_id = null, write_lease_request_id = null, write_leased_until = null, write_lease_recovering = false, updated_at = ",
                        )
                        .bind(Utc::now())
                        .append(" where organization_id = ")
                        .bind(lease.organization_id.as_uuid())
                        .append(" and asset_id = ")
                        .bind(lease.asset_id.as_uuid())
                        .append(" and write_lease_id = ")
                        .bind(lease.lease_id),
                    )
                    .await?,
                )?;
                Ok(())
            })
        })
        .await
        .map_err(map_transaction)
}

pub(super) async fn settle_write(
    executor: &PostgresExecutor,
    journal: &AssetGitWriteJournal,
) -> Result<(), AssetGitRepositoryControlError> {
    if journal.lease_id.is_nil() {
        return Err(AssetGitRepositoryControlError::Invalid(
            "Asset Git write journal identity is invalid".into(),
        ));
    }
    let journal = *journal;
    executor
        .transaction(move |transaction| {
            Box::pin(async move {
                let row = lock_control(
                    transaction,
                    journal.organization_id.as_uuid(),
                    journal.asset_id.as_uuid(),
                )
                .await?
                .ok_or(AssetGitRepositoryControlError::NotFound)?;
                row.validate()?;
                match row.write_cleanup_lease_id {
                    None => return Ok(()),
                    Some(lease_id) if lease_id == journal.lease_id => {}
                    Some(_) => return Err(AssetGitRepositoryControlError::StaleLease.into()),
                }
                require_one_row(
                    "Asset Git write journal settlement",
                    execute(
                        transaction,
                        sql_query::<()>(
                            "update asset_git_repository_controls set write_cleanup_lease_id = null, updated_at = ",
                        )
                        .bind(Utc::now())
                        .append(" where organization_id = ")
                        .bind(journal.organization_id.as_uuid())
                        .append(" and asset_id = ")
                        .bind(journal.asset_id.as_uuid())
                        .append(" and write_cleanup_lease_id = ")
                        .bind(journal.lease_id),
                    )
                    .await?,
                )?;
                Ok(())
            })
        })
        .await
        .map_err(map_transaction)
}

async fn lock_control(
    transaction: &PostgresTransaction,
    organization_id: Uuid,
    asset_id: Uuid,
) -> Result<Option<ControlRow>, PostgresPersistenceError> {
    fetch_optional::<ControlRow, _>(
        transaction,
        sql_query::<ControlRow>(SELECT_CONTROL)
            .append(" where organization_id = ")
            .bind(organization_id)
            .append(" and asset_id = ")
            .bind(asset_id)
            .append(" for update"),
    )
    .await
}

struct ControlRow {
    organization_id: Uuid,
    asset_id: Uuid,
    quota_bytes: u64,
    observed_bytes: u64,
    write_lease_id: Option<Uuid>,
    write_lease_operation: Option<String>,
    write_lease_actor_id: Option<Uuid>,
    write_lease_request_id: Option<Uuid>,
    write_leased_until: Option<DateTime<Utc>>,
    write_lease_recovering: bool,
    write_cleanup_lease_id: Option<Uuid>,
}

impl ControlRow {
    fn validate(&self) -> Result<(), AssetGitRepositoryControlError> {
        if self.organization_id.is_nil()
            || self.asset_id.is_nil()
            || self.quota_bytes == 0
            || self.observed_bytes > self.quota_bytes
            || self
                .write_cleanup_lease_id
                .is_some_and(|lease_id| lease_id.is_nil())
            || (self.write_lease_id.is_some() && self.write_cleanup_lease_id.is_some())
        {
            return Err(AssetGitRepositoryControlError::Storage(
                "stored hosted Git repository control identity is invalid".into(),
            ));
        }
        match (
            self.write_lease_id,
            self.write_lease_operation.as_deref(),
            self.write_lease_actor_id,
            self.write_lease_request_id,
            self.write_leased_until,
        ) {
            (None, None, None, None, None) if !self.write_lease_recovering => Ok(()),
            (Some(id), Some(operation), Some(actor), Some(request), Some(_))
                if !id.is_nil()
                    && !actor.is_nil()
                    && !request.is_nil()
                    && parse_operation(operation).is_some() =>
            {
                Ok(())
            }
            _ => Err(AssetGitRepositoryControlError::Storage(
                "stored hosted Git repository lease is invalid".into(),
            )),
        }
    }

    fn matches(&self, lease: &AssetGitWriteLease) -> bool {
        self.organization_id == lease.organization_id.as_uuid()
            && self.asset_id == lease.asset_id.as_uuid()
            && self.write_lease_id == Some(lease.lease_id)
            && self.write_lease_operation.as_deref() == Some(lease.operation.as_str())
            && self.write_lease_actor_id == Some(lease.actor_id)
            && self.write_lease_request_id == Some(lease.request_id)
            && self.write_lease_recovering == lease.recovery
    }

    fn to_lease(
        &self,
        leased_until: DateTime<Utc>,
        recovery: bool,
    ) -> Result<AssetGitWriteLease, AssetGitRepositoryControlError> {
        Ok(AssetGitWriteLease {
            organization_id: crate::modules::shared_kernel::domain::OrganizationId::from_uuid(
                self.organization_id,
            ),
            asset_id: crate::modules::shared_kernel::domain::AssetId::from_uuid(self.asset_id),
            lease_id: self.write_lease_id.ok_or_else(|| {
                AssetGitRepositoryControlError::Storage(
                    "stored hosted Git repository lease identity is missing".into(),
                )
            })?,
            operation: parse_operation(self.write_lease_operation.as_deref().unwrap_or_default())
                .ok_or_else(|| {
                AssetGitRepositoryControlError::Storage(
                    "stored hosted Git repository lease operation is invalid".into(),
                )
            })?,
            actor_id: self.write_lease_actor_id.ok_or_else(|| {
                AssetGitRepositoryControlError::Storage(
                    "stored hosted Git repository lease actor is missing".into(),
                )
            })?,
            request_id: self.write_lease_request_id.ok_or_else(|| {
                AssetGitRepositoryControlError::Storage(
                    "stored hosted Git repository lease request is missing".into(),
                )
            })?,
            quota_bytes: self.quota_bytes,
            observed_bytes: self.observed_bytes,
            leased_until,
            recovery,
        })
    }
}

impl FromRow for ControlRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            asset_id: decode(row, 1)?,
            quota_bytes: decode(row, 2)?,
            observed_bytes: decode(row, 3)?,
            write_lease_id: decode(row, 4)?,
            write_lease_operation: decode(row, 5)?,
            write_lease_actor_id: decode(row, 6)?,
            write_lease_request_id: decode(row, 7)?,
            write_leased_until: decode(row, 8)?,
            write_lease_recovering: decode(row, 9)?,
            write_cleanup_lease_id: decode(row, 10)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn parse_operation(value: &str) -> Option<AssetGitWriteOperation> {
    match value {
        "receive_pack" => Some(AssetGitWriteOperation::ReceivePack),
        "backup" => Some(AssetGitWriteOperation::Backup),
        "restore" => Some(AssetGitWriteOperation::Restore),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
enum ControlPersistenceError {
    #[error(transparent)]
    Control(#[from] AssetGitRepositoryControlError),
    #[error(transparent)]
    Persistence(#[from] PostgresPersistenceError),
}

fn map_transaction(
    error: PostgresTransactionError<ControlPersistenceError>,
) -> AssetGitRepositoryControlError {
    match error {
        PostgresTransactionError::Operation(ControlPersistenceError::Control(error)) => error,
        PostgresTransactionError::Operation(ControlPersistenceError::Persistence(error)) => {
            AssetGitRepositoryControlError::Storage(error.to_string())
        }
        PostgresTransactionError::Begin(error) => AssetGitRepositoryControlError::Storage(format!(
            "could not begin hosted Git control transaction: {error}"
        )),
        PostgresTransactionError::Commit(error) => AssetGitRepositoryControlError::Storage(
            format!("could not commit hosted Git control transaction: {error}"),
        ),
        PostgresTransactionError::OperationAndRollback {
            operation,
            rollback,
        } => AssetGitRepositoryControlError::Storage(format!(
            "hosted Git control transaction failed ({operation}) and rollback failed ({rollback})"
        )),
    }
}
