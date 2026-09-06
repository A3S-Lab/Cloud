use crate::infrastructure::{
    execute, fetch_all, fetch_optional, is_foreign_key_violation, is_unique_violation,
    require_one_row, store_audit, store_outbox, transaction_error, AuditWrite,
    PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError, Sha256Digest};
use crate::modules::workflow::domain::{
    AppendWorkflowAuthoringOperation, CreateWorkflowAuthoringJournal, IWorkflowAuthoringRepository,
    WorkflowAuthoringAppend, WorkflowAuthoringEntry, WorkflowAuthoringJournal,
    WorkflowAuthoringJournalCreated, WorkflowAuthoringJournalKey,
    WorkflowAuthoringOperationAppended, WorkflowAuthoringPage, WorkflowAuthoringSnapshot,
    WorkflowAuthoringWriteContext, WORKFLOW_AUTHORING_MAX_PAGE_SIZE,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::Utc;

const JOURNAL_SELECT: &str = "select organization_id, project_id, workflow_definition_id, initial_snapshot_bytes, initial_snapshot_digest, current_snapshot_bytes, current_snapshot_digest, next_sequence, created_at, updated_at from workflow_authoring_journals";
const ENTRY_SELECT: &str = "select sequence, operation_id, operation_digest, base_snapshot_digest, result_snapshot_digest, operation_bytes from workflow_authoring_entries";

/// PostgreSQL implementation of the hosted Workflow authoring journal.
///
/// Every append locks the journal head row for the duration of one transaction,
/// checks the operation-id replay before the base CAS, and updates the
/// materialized snapshot and entry in the same transaction.
#[derive(Clone)]
pub struct PostgresWorkflowAuthoringRepository {
    executor: PostgresExecutor,
}

impl PostgresWorkflowAuthoringRepository {
    /// Construct a repository over the Cloud PostgreSQL executor.
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }

    async fn create_impl(
        &self,
        write: CreateWorkflowAuthoringJournal,
        context: Option<WorkflowAuthoringWriteContext>,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .key
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    write
                        .initial_snapshot
                        .validate()
                        .map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "initial workflow authoring snapshot is invalid: {error}"
                            ))
                        })?;
                    if let Some(context) = context {
                        context
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                    }
                    let now = canonical_timestamp(Utc::now());
                    let rows = execute(
                        transaction,
                        sql_query::<()>("insert into workflow_authoring_journals (organization_id, project_id, workflow_definition_id, initial_snapshot_bytes, initial_snapshot_digest, current_snapshot_bytes, current_snapshot_digest, next_sequence, created_at, updated_at) values (")
                            .bind(write.key.organization_id.as_uuid())
                            .append(", ")
                            .bind(write.key.project_id.as_uuid())
                            .append(", ")
                            .bind(write.key.workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(write.initial_snapshot.snapshot_bytes().to_vec())
                            .append(", ")
                            .bind(write.initial_snapshot.snapshot_digest().as_str())
                            .append(", ")
                            .bind(write.initial_snapshot.snapshot_bytes().to_vec())
                            .append(", ")
                            .bind(write.initial_snapshot.snapshot_digest().as_str())
                            .append(", 1, ")
                            .bind(now)
                            .append(", ")
                            .bind(now)
                            .append(")"),
                    )
                    .await;
                    match rows {
                        Ok(rows) => require_one_row("Workflow authoring journal", rows)?,
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Workflow authoring journal already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    if let Some(context) = context {
                        store_created_facts(transaction, write.key, &write.initial_snapshot, context, now)
                            .await?;
                    }
                    WorkflowAuthoringJournal::try_new(write.initial_snapshot).map_err(|error| {
                        PostgresPersistenceError::Invariant(format!(
                            "created workflow authoring journal is invalid: {error}"
                        ))
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn append_impl(
        &self,
        write: AppendWorkflowAuthoringOperation,
        context: Option<WorkflowAuthoringWriteContext>,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    write
                        .key
                        .validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    write
                        .operation
                        .validate()
                        .map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "workflow authoring operation is invalid: {error}"
                            ))
                        })?;
                    if let Some(context) = context {
                        context
                            .validate()
                            .map_err(PostgresPersistenceError::Invariant)?;
                    }
                    let Some((head, current_snapshot)) =
                        load_head(transaction, write.key, true).await?
                    else {
                        return Err(RepositoryError::NotFound.into());
                    };

                    // The head row lock serializes all writers for this
                    // journal. Check the immutable operation index before
                    // validating the supplied result so retries remain safe
                    // even when their result body is omitted or stale.
                    if let Some(row) = find_entry_by_operation_id(
                        transaction,
                        write.key,
                        write.operation.operation_id(),
                    )
                    .await?
                    {
                        let entry = decode_entry(row)?;
                        if entry.operation_digest() != write.operation.operation_digest() {
                            return Err(RepositoryError::IdempotencyConflict.into());
                        }
                        return Ok(WorkflowAuthoringAppend {
                            entry,
                            replayed: true,
                        });
                    }

                    write
                        .result_snapshot
                        .validate()
                        .map_err(|error| {
                            PostgresPersistenceError::Invariant(format!(
                                "workflow authoring result snapshot is invalid: {error}"
                            ))
                        })?;
                    if write.operation.base_snapshot_digest()
                        != current_snapshot.snapshot_digest()
                    {
                        return Err(RepositoryError::Conflict(format!(
                            "workflow authoring base digest mismatch: expected {}, received {}",
                            current_snapshot.snapshot_digest(),
                            write.operation.base_snapshot_digest()
                        ))
                        .into());
                    }
                    let sequence = u64::try_from(head.next_sequence).map_err(|_| {
                        PostgresPersistenceError::Invariant(
                            "workflow authoring next sequence is negative".into(),
                        )
                    })?;
                    if sequence == 0 {
                        return Err(PostgresPersistenceError::Invariant(
                            "workflow authoring next sequence is zero".into(),
                        ));
                    }
                    let next_sequence = sequence.checked_add(1).ok_or_else(|| {
                        PostgresPersistenceError::Invariant(
                            "workflow authoring sequence is exhausted".into(),
                        )
                    })?;
                    if next_sequence > i64::MAX as u64 {
                        return Err(PostgresPersistenceError::Invariant(
                            "workflow authoring sequence exceeds PostgreSQL bigint".into(),
                        ));
                    }
                    let entry = WorkflowAuthoringEntry::try_from_parts(
                        sequence,
                        write.operation.operation_id().to_owned(),
                        write.operation.operation_digest().clone(),
                        write.operation.base_snapshot_digest().clone(),
                        write.result_snapshot.snapshot_digest().clone(),
                        write.operation.operation_bytes().to_vec(),
                    )
                    .map_err(|error| {
                        PostgresPersistenceError::Invariant(format!(
                            "workflow authoring entry is invalid: {error}"
                        ))
                    })?;
                    let inserted = execute(
                        transaction,
                        sql_query::<()>("insert into workflow_authoring_entries (organization_id, project_id, workflow_definition_id, sequence, operation_id, operation_digest, base_snapshot_digest, result_snapshot_digest, operation_bytes) values (")
                            .bind(write.key.organization_id.as_uuid())
                            .append(", ")
                            .bind(write.key.project_id.as_uuid())
                            .append(", ")
                            .bind(write.key.workflow_definition_id.as_uuid())
                            .append(", ")
                            .bind(i64::try_from(sequence).map_err(|_| {
                                PostgresPersistenceError::Invariant(
                                    "workflow authoring sequence exceeds PostgreSQL bigint".into(),
                                )
                            })?)
                            .append(", ")
                            .bind(entry.operation_id())
                            .append(", ")
                            .bind(entry.operation_digest().as_str())
                            .append(", ")
                            .bind(entry.base_snapshot_digest().as_str())
                            .append(", ")
                            .bind(entry.result_snapshot_digest().as_str())
                            .append(", ")
                            .bind(entry.operation_bytes().to_vec())
                            .append(")"),
                    )
                    .await;
                    match inserted {
                        Ok(rows) => require_one_row("Workflow authoring entry", rows)?,
                        Err(error) if is_unique_violation(&error) => {
                            return Err(RepositoryError::Conflict(
                                "Workflow authoring entry already exists".into(),
                            )
                            .into())
                        }
                        Err(error) if is_foreign_key_violation(&error) => {
                            return Err(RepositoryError::NotFound.into())
                        }
                        Err(error) => return Err(error),
                    }
                    let now = canonical_timestamp(Utc::now());
                    let updated = execute(
                        transaction,
                        sql_query::<()>("update workflow_authoring_journals set current_snapshot_bytes = ")
                            .bind(write.result_snapshot.snapshot_bytes().to_vec())
                            .append(", current_snapshot_digest = ")
                            .bind(write.result_snapshot.snapshot_digest().as_str())
                            .append(", next_sequence = ")
                            .bind(i64::try_from(next_sequence).map_err(|_| {
                                PostgresPersistenceError::Invariant(
                                    "workflow authoring sequence exceeds PostgreSQL bigint".into(),
                                )
                            })?)
                            .append(", updated_at = ")
                            .bind(now)
                            .append(" where organization_id = ")
                            .bind(write.key.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(write.key.project_id.as_uuid())
                            .append(" and workflow_definition_id = ")
                            .bind(write.key.workflow_definition_id.as_uuid())
                            .append(" and current_snapshot_digest = ")
                            .bind(write.operation.base_snapshot_digest().as_str()),
                    )
                    .await?;
                    require_one_row("Workflow authoring journal update", updated)?;
                    if let Some(context) = context {
                        store_appended_facts(
                            transaction,
                            write.key,
                            &entry,
                            next_sequence,
                            context,
                            now,
                        )
                        .await?;
                    }
                    Ok(WorkflowAuthoringAppend {
                        entry,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IWorkflowAuthoringRepository for PostgresWorkflowAuthoringRepository {
    async fn create(
        &self,
        write: CreateWorkflowAuthoringJournal,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        self.create_impl(write, None).await
    }

    async fn create_with_context(
        &self,
        write: CreateWorkflowAuthoringJournal,
        context: WorkflowAuthoringWriteContext,
    ) -> Result<WorkflowAuthoringJournal, RepositoryError> {
        self.create_impl(write, Some(context)).await
    }

    async fn append(
        &self,
        write: AppendWorkflowAuthoringOperation,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        self.append_impl(write, None).await
    }

    async fn append_with_context(
        &self,
        write: AppendWorkflowAuthoringOperation,
        context: WorkflowAuthoringWriteContext,
    ) -> Result<WorkflowAuthoringAppend, RepositoryError> {
        self.append_impl(write, Some(context)).await
    }

    async fn current_snapshot(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringSnapshot>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    Ok(load_head(transaction, key, false)
                        .await?
                        .map(|(_, snapshot)| snapshot))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find_operation(
        &self,
        key: WorkflowAuthoringJournalKey,
        operation_id: &str,
    ) -> Result<Option<WorkflowAuthoringEntry>, RepositoryError> {
        let operation_id = operation_id.to_owned();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    key.validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    let Some(row) =
                        find_entry_by_operation_id(transaction, key, &operation_id).await?
                    else {
                        return Ok(None);
                    };
                    decode_entry(row).map(Some)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        key: WorkflowAuthoringJournalKey,
    ) -> Result<Option<WorkflowAuthoringJournal>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { load_journal(transaction, key, false).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn page(
        &self,
        key: WorkflowAuthoringJournalKey,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Option<WorkflowAuthoringPage>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    key.validate()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    if !(1..=WORKFLOW_AUTHORING_MAX_PAGE_SIZE).contains(&limit) {
                        return Err(PostgresPersistenceError::Invariant(format!(
                            "workflow authoring page limit must be between 1 and {WORKFLOW_AUTHORING_MAX_PAGE_SIZE}"
                        )));
                    }
                    let Some((head, _current_snapshot)) = load_head(transaction, key, false).await? else {
                        return Ok(None);
                    };
                    let last_sequence = u64::try_from(head.next_sequence)
                        .ok()
                        .and_then(|value| value.checked_sub(1))
                        .ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "workflow authoring next sequence is invalid".into(),
                            )
                        })?;
                    let after_sequence = after_sequence.unwrap_or(0);
                    if after_sequence > last_sequence {
                        return Err(RepositoryError::Conflict(format!(
                            "workflow authoring cursor is invalid: {after_sequence}"
                        ))
                        .into());
                    }
                    let after_sequence_i64 = i64::try_from(after_sequence).map_err(|_| {
                        PostgresPersistenceError::Invariant(
                            "workflow authoring cursor exceeds PostgreSQL bigint".into(),
                        )
                    })?;
                    let fetch_limit = i64::try_from(limit + 1).map_err(|_| {
                        PostgresPersistenceError::Invariant(
                            "workflow authoring page limit exceeds PostgreSQL bigint".into(),
                        )
                    })?;
                    let rows = fetch_all::<WorkflowAuthoringEntryRow, _>(
                        transaction,
                        sql_query::<WorkflowAuthoringEntryRow>(ENTRY_SELECT)
                            .append(" where organization_id = ")
                            .bind(key.organization_id.as_uuid())
                            .append(" and project_id = ")
                            .bind(key.project_id.as_uuid())
                            .append(" and workflow_definition_id = ")
                            .bind(key.workflow_definition_id.as_uuid())
                            .append(" and sequence > ")
                            .bind(after_sequence_i64)
                            .append(" order by sequence asc limit ")
                            .bind(fetch_limit),
                    )
                    .await?;
                    let has_more = rows.len() > limit;
                    let entries = rows
                        .into_iter()
                        .take(limit)
                        .map(decode_entry)
                        .collect::<Result<Vec<_>, _>>()?;
                    let mut expected_sequence = after_sequence.saturating_add(1);
                    for entry in &entries {
                        if entry.sequence() != expected_sequence {
                            return Err(PostgresPersistenceError::Invariant(
                                "workflow authoring entry sequences are not contiguous".into(),
                            ));
                        }
                        expected_sequence = expected_sequence.checked_add(1).ok_or_else(|| {
                            PostgresPersistenceError::Invariant(
                                "workflow authoring page sequence is exhausted".into(),
                            )
                        })?;
                    }
                    if !has_more
                        && ((!entries.is_empty()
                            && entries.last().map(WorkflowAuthoringEntry::sequence)
                                != Some(last_sequence))
                            || (entries.is_empty() && after_sequence != last_sequence))
                    {
                        return Err(PostgresPersistenceError::Invariant(
                            "workflow authoring page does not reach the journal head".into(),
                        ));
                    }
                    let next_cursor = has_more
                        .then(|| entries.last().map(WorkflowAuthoringEntry::sequence))
                        .flatten();
                    Ok(Some(WorkflowAuthoringPage {
                        entries,
                        next_cursor,
                    }))
                })
            })
            .await
            .map_err(transaction_error)
    }
}

#[derive(Debug)]
struct WorkflowAuthoringJournalRow {
    organization_id: uuid::Uuid,
    project_id: uuid::Uuid,
    workflow_definition_id: uuid::Uuid,
    initial_snapshot_bytes: Vec<u8>,
    initial_snapshot_digest: String,
    current_snapshot_bytes: Vec<u8>,
    current_snapshot_digest: String,
    next_sequence: i64,
    _created_at: chrono::DateTime<Utc>,
    _updated_at: chrono::DateTime<Utc>,
}

impl FromRow for WorkflowAuthoringJournalRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            workflow_definition_id: decode(row, 2)?,
            initial_snapshot_bytes: decode(row, 3)?,
            initial_snapshot_digest: decode(row, 4)?,
            current_snapshot_bytes: decode(row, 5)?,
            current_snapshot_digest: decode(row, 6)?,
            next_sequence: decode(row, 7)?,
            _created_at: decode(row, 8)?,
            _updated_at: decode(row, 9)?,
        })
    }
}

#[derive(Debug)]
struct WorkflowAuthoringEntryRow {
    sequence: i64,
    operation_id: String,
    operation_digest: String,
    base_snapshot_digest: String,
    result_snapshot_digest: String,
    operation_bytes: Vec<u8>,
}

impl FromRow for WorkflowAuthoringEntryRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            sequence: decode(row, 0)?,
            operation_id: decode(row, 1)?,
            operation_digest: decode(row, 2)?,
            base_snapshot_digest: decode(row, 3)?,
            result_snapshot_digest: decode(row, 4)?,
            operation_bytes: decode(row, 5)?,
        })
    }
}

async fn load_head(
    transaction: &PostgresTransaction,
    key: WorkflowAuthoringJournalKey,
    for_update: bool,
) -> Result<
    Option<(WorkflowAuthoringJournalRow, WorkflowAuthoringSnapshot)>,
    PostgresPersistenceError,
> {
    key.validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let mut query = sql_query::<WorkflowAuthoringJournalRow>(JOURNAL_SELECT)
        .append(" where organization_id = ")
        .bind(key.organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(key.project_id.as_uuid())
        .append(" and workflow_definition_id = ")
        .bind(key.workflow_definition_id.as_uuid());
    if for_update {
        query = query.append(" for update");
    }
    let Some(row) = fetch_optional(transaction, query).await? else {
        return Ok(None);
    };
    if row.organization_id != key.organization_id.as_uuid()
        || row.project_id != key.project_id.as_uuid()
        || row.workflow_definition_id != key.workflow_definition_id.as_uuid()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored workflow authoring journal identity does not match its key".into(),
        ));
    }
    let snapshot = decode_snapshot(
        row.current_snapshot_bytes.clone(),
        row.current_snapshot_digest.clone(),
        "current",
    )?;
    Ok(Some((row, snapshot)))
}

async fn load_journal(
    transaction: &PostgresTransaction,
    key: WorkflowAuthoringJournalKey,
    for_update: bool,
) -> Result<Option<WorkflowAuthoringJournal>, PostgresPersistenceError> {
    let Some((row, current_snapshot)) = load_head(transaction, key, for_update).await? else {
        return Ok(None);
    };
    let initial_snapshot = decode_snapshot(
        row.initial_snapshot_bytes,
        row.initial_snapshot_digest,
        "initial",
    )?;
    let next_sequence = u64::try_from(row.next_sequence).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "stored workflow authoring next sequence is negative".into(),
        )
    })?;
    let rows = fetch_all::<WorkflowAuthoringEntryRow, _>(
        transaction,
        sql_query::<WorkflowAuthoringEntryRow>(ENTRY_SELECT)
            .append(" where organization_id = ")
            .bind(key.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(key.project_id.as_uuid())
            .append(" and workflow_definition_id = ")
            .bind(key.workflow_definition_id.as_uuid())
            .append(" order by sequence asc"),
    )
    .await?;
    let entries = rows
        .into_iter()
        .map(decode_entry)
        .collect::<Result<Vec<_>, _>>()?;
    WorkflowAuthoringJournal::try_from_parts(
        initial_snapshot,
        current_snapshot,
        next_sequence,
        entries,
    )
    .map(Some)
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring journal is invalid: {error}"
        ))
    })
}

async fn find_entry_by_operation_id(
    transaction: &PostgresTransaction,
    key: WorkflowAuthoringJournalKey,
    operation_id: &str,
) -> Result<Option<WorkflowAuthoringEntryRow>, PostgresPersistenceError> {
    fetch_optional(
        transaction,
        sql_query::<WorkflowAuthoringEntryRow>(ENTRY_SELECT)
            .append(" where organization_id = ")
            .bind(key.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(key.project_id.as_uuid())
            .append(" and workflow_definition_id = ")
            .bind(key.workflow_definition_id.as_uuid())
            .append(" and operation_id = ")
            .bind(operation_id),
    )
    .await
}

async fn store_created_facts(
    transaction: &PostgresTransaction,
    key: WorkflowAuthoringJournalKey,
    snapshot: &WorkflowAuthoringSnapshot,
    context: WorkflowAuthoringWriteContext,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let event =
        WorkflowAuthoringJournalCreated::envelope(key, snapshot, context.request_id, occurred_at)?;
    store_outbox(transaction, &event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: uuid::Uuid::now_v7(),
            scope: AuditWrite::resource_scope(key.organization_id.as_uuid(), key.project_id, None),
            actor_id: Some(context.actor_principal_id.as_uuid()),
            action: "workflow.authoring.created",
            aggregate_id: key.workflow_definition_id.as_uuid(),
            occurred_at,
            request_id: context.request_id,
            details: serde_json::json!({
                "projectId": key.project_id,
                "workflowDefinitionId": key.workflow_definition_id,
                "snapshotDigest": snapshot.snapshot_digest(),
                "aggregateVersion": 1,
            }),
        },
    )
    .await
}

async fn store_appended_facts(
    transaction: &PostgresTransaction,
    key: WorkflowAuthoringJournalKey,
    entry: &WorkflowAuthoringEntry,
    aggregate_version: u64,
    context: WorkflowAuthoringWriteContext,
    occurred_at: chrono::DateTime<Utc>,
) -> Result<(), PostgresPersistenceError> {
    let event = WorkflowAuthoringOperationAppended::envelope(
        key,
        entry,
        aggregate_version,
        context.request_id,
        occurred_at,
    )?;
    store_outbox(transaction, &event).await?;
    store_audit(
        transaction,
        &AuditWrite {
            audit_id: uuid::Uuid::now_v7(),
            scope: AuditWrite::resource_scope(key.organization_id.as_uuid(), key.project_id, None),
            actor_id: Some(context.actor_principal_id.as_uuid()),
            action: "workflow.authoring.operation-appended",
            aggregate_id: key.workflow_definition_id.as_uuid(),
            occurred_at,
            request_id: context.request_id,
            details: serde_json::json!({
                "projectId": key.project_id,
                "workflowDefinitionId": key.workflow_definition_id,
                "sequence": entry.sequence(),
                "operationId": entry.operation_id(),
                "operationDigest": entry.operation_digest(),
                "baseSnapshotDigest": entry.base_snapshot_digest(),
                "resultSnapshotDigest": entry.result_snapshot_digest(),
                "aggregateVersion": aggregate_version,
            }),
        },
    )
    .await
}

fn decode_snapshot(
    bytes: Vec<u8>,
    digest: String,
    label: &str,
) -> Result<WorkflowAuthoringSnapshot, PostgresPersistenceError> {
    let digest = Sha256Digest::parse(digest).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring {label} snapshot digest is invalid: {error}"
        ))
    })?;
    WorkflowAuthoringSnapshot::try_from_verified_bytes(bytes, digest).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring {label} snapshot is invalid: {error}"
        ))
    })
}

fn decode_entry(
    row: WorkflowAuthoringEntryRow,
) -> Result<WorkflowAuthoringEntry, PostgresPersistenceError> {
    let sequence = u64::try_from(row.sequence).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "stored workflow authoring entry sequence is negative".into(),
        )
    })?;
    let operation_digest = Sha256Digest::parse(row.operation_digest).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring operation digest is invalid: {error}"
        ))
    })?;
    let base_snapshot_digest = Sha256Digest::parse(row.base_snapshot_digest).map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring base snapshot digest is invalid: {error}"
        ))
    })?;
    let result_snapshot_digest =
        Sha256Digest::parse(row.result_snapshot_digest).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored workflow authoring result snapshot digest is invalid: {error}"
            ))
        })?;
    WorkflowAuthoringEntry::try_from_parts(
        sequence,
        row.operation_id,
        operation_digest,
        base_snapshot_digest,
        result_snapshot_digest,
        row.operation_bytes,
    )
    .map_err(|error| {
        PostgresPersistenceError::Invariant(format!(
            "stored workflow authoring entry is invalid: {error}"
        ))
    })
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn append_sql_locks_the_head_before_writing_the_entry() {
        assert!(super::JOURNAL_SELECT.contains("from workflow_authoring_journals"));
        assert!(super::ENTRY_SELECT.contains("from workflow_authoring_entries"));
    }
}
