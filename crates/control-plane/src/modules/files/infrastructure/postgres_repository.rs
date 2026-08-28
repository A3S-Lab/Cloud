use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_idempotency, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::files::domain::{
    IUserFileRepository, ReserveUserFileWrite, TransitionUserFileWrite, UserFile, UserFileQuota,
    UserFileState, DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES, USER_FILE_ADMISSION_CONTRACT_SCHEMA,
};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, ProjectId, RepositoryError,
    Sha256Digest, UserFileId, UserFileUploadId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor,
    PostgresTransaction, Row, SqlQuery,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_USER_FILES: &str = "select organization_id, project_id, id, upload_id, canonical_acl, contract_digest, size_bytes, upload_expires_at, retention_until, state, scan_evidence_digest, rejection_reason_code, tombstoned_from, aggregate_version, created_by, created_at, uploaded_at, scanned_at, expired_at, tombstoned_at, updated_at, cleanup_due_at from user_files";
const SELECT_USER_FILE_QUOTAS: &str = "select organization_id, limit_bytes, allocated_bytes, revision, updated_at from user_file_organization_quotas";

struct UserFileRow {
    organization_id: Uuid,
    project_id: Uuid,
    id: Uuid,
    upload_id: Uuid,
    canonical_acl: String,
    contract_digest: String,
    size_bytes: u64,
    upload_expires_at: DateTime<Utc>,
    retention_until: DateTime<Utc>,
    state: String,
    scan_evidence_digest: Option<String>,
    rejection_reason_code: Option<String>,
    tombstoned_from: Option<String>,
    aggregate_version: u64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    uploaded_at: Option<DateTime<Utc>>,
    scanned_at: Option<DateTime<Utc>>,
    expired_at: Option<DateTime<Utc>>,
    tombstoned_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    cleanup_due_at: Option<DateTime<Utc>>,
}

impl FromRow for UserFileRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            id: decode(row, 2)?,
            upload_id: decode(row, 3)?,
            canonical_acl: decode(row, 4)?,
            contract_digest: decode(row, 5)?,
            size_bytes: decode(row, 6)?,
            upload_expires_at: decode(row, 7)?,
            retention_until: decode(row, 8)?,
            state: decode(row, 9)?,
            scan_evidence_digest: decode(row, 10)?,
            rejection_reason_code: decode(row, 11)?,
            tombstoned_from: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
            created_by: decode(row, 14)?,
            created_at: decode(row, 15)?,
            uploaded_at: decode(row, 16)?,
            scanned_at: decode(row, 17)?,
            expired_at: decode(row, 18)?,
            tombstoned_at: decode(row, 19)?,
            updated_at: decode(row, 20)?,
            cleanup_due_at: decode(row, 21)?,
        })
    }
}

struct UserFileQuotaRow {
    organization_id: Uuid,
    limit_bytes: u64,
    allocated_bytes: u64,
    revision: u64,
    updated_at: Option<DateTime<Utc>>,
}

impl FromRow for UserFileQuotaRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            limit_bytes: decode(row, 1)?,
            allocated_bytes: decode(row, 2)?,
            revision: decode(row, 3)?,
            updated_at: decode(row, 4)?,
        })
    }
}

#[derive(Clone)]
pub struct PostgresUserFileRepository {
    executor: PostgresExecutor,
    default_quota_bytes: u64,
}

impl PostgresUserFileRepository {
    pub fn new(executor: PostgresExecutor) -> Self {
        Self {
            executor,
            default_quota_bytes: DEFAULT_USER_FILE_ORGANIZATION_QUOTA_BYTES,
        }
    }
}

#[async_trait]
impl IUserFileRepository for PostgresUserFileRepository {
    async fn replay_write(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<IdempotentWrite<UserFile>>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let replay = idempotency_replay::<UserFile>(transaction, &idempotency).await?;
                    if let Some(replay) = &replay {
                        replay
                            .value
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                    }
                    Ok(replay)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn reserve(
        &self,
        write: ReserveUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        let default_quota_bytes = self.default_quota_bytes;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<UserFile>(transaction, &write.idempotency).await?
                    {
                        replay
                            .value
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                        return Ok(replay);
                    }
                    ensure_quota(transaction, write.file.organization_id, default_quota_bytes)
                        .await?;
                    let quota = lock_quota(transaction, write.file.organization_id).await?;
                    let size_bytes = write.file.contract.spec().content.size_bytes;
                    if !quota.can_reserve(size_bytes) {
                        return Err(quota_exceeded(&quota, size_bytes).into());
                    }
                    let next_quota = quota
                        .reserve(size_bytes, write.file.updated_at)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    update_quota(transaction, &quota, &next_quota).await?;
                    match insert_file(transaction, &write.file).await {
                        Ok(()) => {}
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "UserFile identity is already reserved".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    persist_side_effects(
                        transaction,
                        &write.file,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        write.audit_action(),
                        false,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.file,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn transition(
        &self,
        write: TransitionUserFileWrite,
    ) -> Result<IdempotentWrite<UserFile>, RepositoryError> {
        write.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<UserFile>(transaction, &write.idempotency).await?
                    {
                        replay
                            .value
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                        return Ok(replay);
                    }
                    let current = lock_file(
                        transaction,
                        write.file.organization_id,
                        write.file.project_id,
                        write.file.id,
                    )
                    .await?;
                    write.validate_against(&current).map_err(|error| {
                        PostgresPersistenceError::Repository(RepositoryError::Conflict(error))
                    })?;
                    let quota_released = current.quota_reserved() && !write.file.quota_reserved();
                    if quota_released {
                        let quota = lock_quota(transaction, write.file.organization_id).await?;
                        let next_quota = quota
                            .release(
                                current.contract.spec().content.size_bytes,
                                write.file.updated_at,
                            )
                            .map_err(PostgresPersistenceError::Invariant)?;
                        update_quota(transaction, &quota, &next_quota).await?;
                    }
                    update_file(transaction, &write.file, write.expected_version).await?;
                    persist_side_effects(
                        transaction,
                        &write.file,
                        &write.event,
                        write.actor_principal_id,
                        write.request_id,
                        &write.idempotency,
                        write.audit_action(),
                        quota_released,
                    )
                    .await?;
                    Ok(IdempotentWrite {
                        value: write.file,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        user_file_id: UserFileId,
    ) -> Result<Option<UserFile>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                user_file_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and id = ")
                    .bind(user_file_id.as_uuid()),
            )
            .await
            .map_err(storage)?
            .map(decode_file)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<UserFile>, RepositoryError> {
        if limit == 0 {
            return Err(RepositoryError::Storage(
                "UserFile list limit must be positive".into(),
            ));
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                user_file_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" order by created_at asc, id asc limit ")
                    .bind(limit as u64),
            )
            .await
            .map_err(storage)?
            .rows
            .into_iter()
            .map(decode_file)
            .collect()
    }

    async fn quota(
        &self,
        organization_id: OrganizationId,
    ) -> Result<UserFileQuota, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                quota_select()
                    .append(" where organization_id = ")
                    .bind(organization_id.as_uuid()),
            )
            .await
            .map_err(storage)?;
        match row {
            Some(row) => decode_quota(row),
            None => UserFileQuota::empty(organization_id, self.default_quota_bytes)
                .map_err(RepositoryError::Storage),
        }
    }
}

fn user_file_select() -> SqlQuery<UserFileRow> {
    sql_query::<UserFileRow>(SELECT_USER_FILES)
}

fn quota_select() -> SqlQuery<UserFileQuotaRow> {
    sql_query::<UserFileQuotaRow>(SELECT_USER_FILE_QUOTAS)
}

async fn lock_file(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    project_id: ProjectId,
    user_file_id: UserFileId,
) -> Result<UserFile, PostgresPersistenceError> {
    fetch_optional::<UserFileRow, _>(
        transaction,
        user_file_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(project_id.as_uuid())
            .append(" and id = ")
            .bind(user_file_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_file)
    .transpose()?
    .ok_or_else(|| RepositoryError::NotFound.into())
}

async fn ensure_quota(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    default_quota_bytes: u64,
) -> Result<(), PostgresPersistenceError> {
    match execute(
        transaction,
        sql_query::<()>("insert into user_file_organization_quotas (organization_id, limit_bytes, allocated_bytes, revision, updated_at) values (")
            .bind(organization_id.as_uuid())
            .append(", ")
            .bind(default_quota_bytes)
            .append(", 0, 0, null) on conflict (organization_id) do nothing"),
    )
    .await
    {
        Ok(0 | 1) => Ok(()),
        Ok(rows) => Err(PostgresPersistenceError::Invariant(format!(
            "ensuring UserFile quota affected {rows} rows"
        ))),
        Err(error) if is_foreign_key_violation(&error) => Err(RepositoryError::NotFound.into()),
        Err(error) => Err(error),
    }
}

async fn lock_quota(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
) -> Result<UserFileQuota, PostgresPersistenceError> {
    fetch_optional::<UserFileQuotaRow, _>(
        transaction,
        quota_select()
            .append(" where organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .map(decode_quota)
    .transpose()?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "UserFile allocation has no organization quota row".into(),
        )
    })
}

async fn update_quota(
    transaction: &PostgresTransaction,
    current: &UserFileQuota,
    next: &UserFileQuota,
) -> Result<(), PostgresPersistenceError> {
    if current.organization_id != next.organization_id
        || current.limit_bytes != next.limit_bytes
        || next.revision != current.revision.saturating_add(1)
    {
        return Err(PostgresPersistenceError::Invariant(
            "UserFile quota successor is invalid".into(),
        ));
    }
    require_one_row(
        "UserFile organization quota",
        execute(
            transaction,
            sql_query::<()>("update user_file_organization_quotas set allocated_bytes = ")
                .bind(next.allocated_bytes)
                .append(", revision = ")
                .bind(next.revision)
                .append(", updated_at = ")
                .bind(next.updated_at)
                .append(" where organization_id = ")
                .bind(current.organization_id.as_uuid())
                .append(" and limit_bytes = ")
                .bind(current.limit_bytes)
                .append(" and allocated_bytes = ")
                .bind(current.allocated_bytes)
                .append(" and revision = ")
                .bind(current.revision),
        )
        .await?,
    )
}

async fn insert_file(
    transaction: &PostgresTransaction,
    file: &UserFile,
) -> Result<(), PostgresPersistenceError> {
    file.validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    require_one_row(
        "UserFile",
        execute(
            transaction,
            sql_query::<()>("insert into user_files (organization_id, project_id, id, upload_id, contract_schema, canonical_acl, contract_digest, size_bytes, upload_expires_at, retention_until, state, scan_evidence_digest, rejection_reason_code, tombstoned_from, aggregate_version, created_by, created_at, uploaded_at, scanned_at, expired_at, tombstoned_at, updated_at, cleanup_due_at) values (")
                .bind(file.organization_id.as_uuid())
                .append(", ")
                .bind(file.project_id.as_uuid())
                .append(", ")
                .bind(file.id.as_uuid())
                .append(", ")
                .bind(file.upload_id.as_uuid())
                .append(", ")
                .bind(USER_FILE_ADMISSION_CONTRACT_SCHEMA)
                .append(", ")
                .bind(file.contract.canonical_acl())
                .append(", ")
                .bind(file.contract.digest().as_str())
                .append(", ")
                .bind(file.contract.spec().content.size_bytes)
                .append(", ")
                .bind(file.contract.spec().upload_expires_at)
                .append(", ")
                .bind(file.contract.spec().retention_until)
                .append(", ")
                .bind(file.state.as_str())
                .append(", ")
                .bind(file.scan_evidence_digest.as_ref().map(Sha256Digest::as_str))
                .append(", ")
                .bind(file.rejection_reason_code.as_deref())
                .append(", ")
                .bind(file.tombstoned_from.map(UserFileState::as_str))
                .append(", ")
                .bind(file.aggregate_version)
                .append(", ")
                .bind(file.created_by.as_uuid())
                .append(", ")
                .bind(file.created_at)
                .append(", ")
                .bind(file.uploaded_at)
                .append(", ")
                .bind(file.scanned_at)
                .append(", ")
                .bind(file.expired_at)
                .append(", ")
                .bind(file.tombstoned_at)
                .append(", ")
                .bind(file.updated_at)
                .append(", ")
                .bind(file.cleanup_due_at())
                .append(")"),
        )
        .await?,
    )
}

async fn update_file(
    transaction: &PostgresTransaction,
    file: &UserFile,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>("update user_files set state = ")
            .bind(file.state.as_str())
            .append(", scan_evidence_digest = ")
            .bind(file.scan_evidence_digest.as_ref().map(Sha256Digest::as_str))
            .append(", rejection_reason_code = ")
            .bind(file.rejection_reason_code.as_deref())
            .append(", tombstoned_from = ")
            .bind(file.tombstoned_from.map(UserFileState::as_str))
            .append(", aggregate_version = ")
            .bind(file.aggregate_version)
            .append(", uploaded_at = ")
            .bind(file.uploaded_at)
            .append(", scanned_at = ")
            .bind(file.scanned_at)
            .append(", expired_at = ")
            .bind(file.expired_at)
            .append(", tombstoned_at = ")
            .bind(file.tombstoned_at)
            .append(", updated_at = ")
            .bind(file.updated_at)
            .append(", cleanup_due_at = ")
            .bind(file.cleanup_due_at())
            .append(" where organization_id = ")
            .bind(file.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(file.project_id.as_uuid())
            .append(" and id = ")
            .bind(file.id.as_uuid())
            .append(" and aggregate_version = ")
            .bind(expected_version),
    )
    .await?;
    match rows {
        1 => Ok(()),
        0 => Err(RepositoryError::Conflict(
            "UserFile was changed from a stale aggregate version".into(),
        )
        .into()),
        rows => Err(PostgresPersistenceError::Invariant(format!(
            "updating UserFile affected {rows} rows"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
async fn persist_side_effects(
    transaction: &PostgresTransaction,
    file: &UserFile,
    event: &DomainEventEnvelope,
    actor_principal_id: PrincipalId,
    request_id: Uuid,
    idempotency: &IdempotencyRequest,
    action: &'static str,
    quota_released: bool,
) -> Result<(), PostgresPersistenceError> {
    store_outbox(transaction, event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: Uuid::now_v7(),
            actor_id: Some(actor_principal_id.as_uuid()),
            action,
            aggregate_id: file.id.as_uuid(),
            occurred_at: file.updated_at,
            request_id,
            scope: AuditWrite::resource_scope(
                file.organization_id.as_uuid(),
                file.project_id,
                None,
            ),
            details: serde_json::json!({
                "projectId": file.project_id,
                "userFileId": file.id,
                "uploadId": file.upload_id,
                "state": file.state.as_str(),
                "contractSchema": USER_FILE_ADMISSION_CONTRACT_SCHEMA,
                "contractDigest": file.contract.digest(),
                "contentDigest": file.contract.spec().content.digest,
                "sizeBytes": file.contract.spec().content.size_bytes,
                "mediaType": file.contract.spec().content.media_type,
                "quotaReleased": quota_released,
                "cleanupDueAt": file.cleanup_due_at(),
            }),
        },
    )
    .await?;
    store_idempotency(transaction, idempotency, file).await
}

fn decode_file(row: UserFileRow) -> Result<UserFile, RepositoryError> {
    let state = parse_state(&row.state)?;
    let tombstoned_from = row
        .tombstoned_from
        .as_deref()
        .map(parse_state)
        .transpose()?;
    let scan_evidence_digest = row
        .scan_evidence_digest
        .map(Sha256Digest::parse)
        .transpose()
        .map_err(stored("UserFile scan evidence"))?;
    let file = UserFile::restore(
        OrganizationId::from_uuid(row.organization_id),
        ProjectId::from_uuid(row.project_id),
        UserFileId::from_uuid(row.id),
        UserFileUploadId::from_uuid(row.upload_id),
        &row.canonical_acl,
        &row.contract_digest,
        state,
        scan_evidence_digest,
        row.rejection_reason_code,
        tombstoned_from,
        row.aggregate_version,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
        row.uploaded_at,
        row.scanned_at,
        row.expired_at,
        row.tombstoned_at,
        row.updated_at,
    )
    .map_err(stored("UserFile aggregate"))?;
    if row.size_bytes != file.contract.spec().content.size_bytes
        || row.upload_expires_at != file.contract.spec().upload_expires_at
        || row.retention_until != file.contract.spec().retention_until
        || row.cleanup_due_at != file.cleanup_due_at()
    {
        return Err(RepositoryError::Storage(
            "stored UserFile quota or cleanup projection drifted from its ACL lifecycle".into(),
        ));
    }
    Ok(file)
}

fn decode_quota(row: UserFileQuotaRow) -> Result<UserFileQuota, RepositoryError> {
    UserFileQuota::restore(
        OrganizationId::from_uuid(row.organization_id),
        row.limit_bytes,
        row.allocated_bytes,
        row.revision,
        row.updated_at,
    )
    .map_err(stored("UserFile organization quota"))
}

fn parse_state(value: &str) -> Result<UserFileState, RepositoryError> {
    match value {
        "awaiting_upload" => Ok(UserFileState::AwaitingUpload),
        "awaiting_scan" => Ok(UserFileState::AwaitingScan),
        "admitted" => Ok(UserFileState::Admitted),
        "rejected" => Ok(UserFileState::Rejected),
        "expired" => Ok(UserFileState::Expired),
        "tombstoned" => Ok(UserFileState::Tombstoned),
        _ => Err(RepositoryError::Storage(
            "stored UserFile state is unsupported".into(),
        )),
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn stored(label: &'static str) -> impl FnOnce(String) -> RepositoryError {
    move |error| RepositoryError::Storage(format!("stored {label} is invalid: {error}"))
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn quota_exceeded(quota: &UserFileQuota, requested_bytes: u64) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "UserFile organization quota exceeded: requested {requested_bytes} bytes with {} bytes available",
        quota.available_bytes()
    ))
}
