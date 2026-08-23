use crate::infrastructure::{
    execute, fetch_all, fetch_optional, require_one_row, transaction_error, AuditRecords,
    AuditRetentionStates, PostgresPersistenceError,
};
use crate::modules::audit::domain::{
    validate_retained_query_window, AuditAttributionStatus, AuditExportSnapshot, AuditRecord,
    AuditRecordCursor, AuditRecordFilter, AuditRetentionReport, AuditRetentionState,
    AuditRetentionSweep, IAuditRecordRepository,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectAttributionProfileId, ProjectId,
    RepositoryError, Sha256Digest,
};
use a3s_orm::expression::Selection;
use a3s_orm::{
    delete_from, select_from, update_table, Database, DecodeError, Expression, FromRow, FromValue,
    OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

struct AuditRecordRow {
    id: Uuid,
    organization_id: Uuid,
    actor_principal_id: Option<Uuid>,
    action: String,
    aggregate_id: Uuid,
    occurred_at: DateTime<Utc>,
    request_id: Uuid,
    project_id: Option<Uuid>,
    environment_id: Option<Uuid>,
    attribution_profile_id: Option<Uuid>,
    attribution_status: String,
}

impl FromRow for AuditRecordRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            id: decode_column(row, 0)?,
            organization_id: decode_column(row, 1)?,
            actor_principal_id: decode_column(row, 2)?,
            action: decode_column(row, 3)?,
            aggregate_id: decode_column(row, 4)?,
            occurred_at: decode_column(row, 5)?,
            request_id: decode_column(row, 6)?,
            project_id: decode_column(row, 7)?,
            environment_id: decode_column(row, 8)?,
            attribution_profile_id: decode_column(row, 9)?,
            attribution_status: decode_column(row, 10)?,
        })
    }
}

struct AuditRecordSelection;

impl Selection for AuditRecordSelection {
    type Output = AuditRecordRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AuditRecords::audit_id().expression(),
            AuditRecords::organization_id().expression(),
            AuditRecords::actor_id().expression(),
            AuditRecords::action().expression(),
            AuditRecords::aggregate_id().expression(),
            AuditRecords::occurred_at().expression(),
            AuditRecords::request_id().expression(),
            AuditRecords::project_id().expression(),
            AuditRecords::environment_id().expression(),
            AuditRecords::attribution_profile_id().expression(),
            AuditRecords::attribution_status().expression(),
        ]
    }
}

struct AuditRetentionStateRow {
    organization_id: Uuid,
    records_available_from: Option<DateTime<Utc>>,
    records_deleted_before: Option<DateTime<Utc>>,
    applied_policy_digest: Option<String>,
    total_deleted_records: u64,
    last_swept_at: Option<DateTime<Utc>>,
    last_completed_at: Option<DateTime<Utc>>,
    next_scan_at: DateTime<Utc>,
    version: u64,
}

impl FromRow for AuditRetentionStateRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode_column(row, 0)?,
            records_available_from: decode_column(row, 1)?,
            records_deleted_before: decode_column(row, 2)?,
            applied_policy_digest: decode_column(row, 3)?,
            total_deleted_records: decode_column(row, 4)?,
            last_swept_at: decode_column(row, 5)?,
            last_completed_at: decode_column(row, 6)?,
            next_scan_at: decode_column(row, 7)?,
            version: decode_column(row, 8)?,
        })
    }
}

struct AuditRetentionStateSelection;

impl Selection for AuditRetentionStateSelection {
    type Output = AuditRetentionStateRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            AuditRetentionStates::organization_id().expression(),
            AuditRetentionStates::records_available_from().expression(),
            AuditRetentionStates::records_deleted_before().expression(),
            AuditRetentionStates::applied_policy_digest().expression(),
            AuditRetentionStates::total_deleted_records().expression(),
            AuditRetentionStates::last_swept_at().expression(),
            AuditRetentionStates::last_completed_at().expression(),
            AuditRetentionStates::next_scan_at().expression(),
            AuditRetentionStates::version().expression(),
        ]
    }
}

#[derive(Clone)]
pub struct PostgresAuditRecordRepository {
    executor: PostgresExecutor,
}

impl PostgresAuditRecordRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAuditRecordRepository for PostgresAuditRecordRepository {
    async fn list_page(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        after: Option<AuditRecordCursor>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>, RepositoryError> {
        let filter = filter.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let state = load_retention_state_for_share(transaction, organization_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    validate_retained_query_window(state.records_available_from, &filter, after)
                        .map_err(RepositoryError::Conflict)?;
                    query_records(
                        transaction,
                        organization_id,
                        &filter,
                        after,
                        state.records_available_from,
                        limit,
                    )
                    .await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn retention_state(
        &self,
        organization_id: OrganizationId,
    ) -> Result<AuditRetentionState, RepositoryError> {
        let row = Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                retention_state_query()
                    .filter(AuditRetentionStates::organization_id().eq(organization_id.as_uuid())),
            )
            .await
            .map_err(storage)?
            .ok_or(RepositoryError::NotFound)?;
        decode_retention_state(row)
    }

    async fn capture_export_snapshot(
        &self,
        organization_id: OrganizationId,
        filter: &AuditRecordFilter,
        maximum_records: usize,
    ) -> Result<AuditExportSnapshot, RepositoryError> {
        if maximum_records == 0 {
            return Err(RepositoryError::Storage(
                "audit export snapshot bound must be positive".into(),
            ));
        }
        let filter = filter.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let state = load_retention_state_for_update(transaction, organization_id)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    validate_retained_query_window(state.records_available_from, &filter, None)
                        .map_err(RepositoryError::Conflict)?;
                    let records = query_records(
                        transaction,
                        organization_id,
                        &filter,
                        None,
                        state.records_available_from,
                        maximum_records,
                    )
                    .await?;
                    let snapshot = AuditExportSnapshot {
                        retention_state: state,
                        records,
                    };
                    snapshot
                        .validate(organization_id, &filter, maximum_records)
                        .map_err(PostgresPersistenceError::Invariant)?;
                    Ok(snapshot)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn sweep_retention(
        &self,
        sweep: AuditRetentionSweep,
    ) -> Result<AuditRetentionReport, RepositoryError> {
        sweep.validate().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(sweep_retention_in_transaction(transaction, sweep))
            })
            .await
            .map_err(transaction_error)
    }
}

async fn query_records(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    filter: &AuditRecordFilter,
    after: Option<AuditRecordCursor>,
    records_available_from: Option<DateTime<Utc>>,
    limit: usize,
) -> Result<Vec<AuditRecord>, PostgresPersistenceError> {
    let mut query = select_from::<AuditRecords>()
        .select(AuditRecordSelection)
        .filter(AuditRecords::organization_id().eq(organization_id.as_uuid()));
    if let Some(boundary) = records_available_from {
        query = query.filter(AuditRecords::occurred_at().gte(boundary));
    }
    if let Some(actor_id) = filter.actor_principal_id {
        query = query.filter(AuditRecords::actor_id().eq(Some(actor_id.as_uuid())));
    }
    if let Some(action) = &filter.action {
        query = query.filter(AuditRecords::action().eq(action.clone()));
    }
    if let Some(aggregate_id) = filter.aggregate_id {
        query = query.filter(AuditRecords::aggregate_id().eq(aggregate_id));
    }
    if let Some(request_id) = filter.request_id {
        query = query.filter(AuditRecords::request_id().eq(request_id));
    }
    if let Some(project_id) = filter.project_id {
        query = query.filter(AuditRecords::project_id().eq(Some(project_id.as_uuid())));
    }
    if let Some(environment_id) = filter.environment_id {
        query = query.filter(AuditRecords::environment_id().eq(Some(environment_id.as_uuid())));
    }
    if let Some(profile_id) = filter.attribution_profile_id {
        query = query.filter(AuditRecords::attribution_profile_id().eq(Some(profile_id.as_uuid())));
    }
    if let Some(status) = filter.attribution_status {
        query = query.filter(AuditRecords::attribution_status().eq(status.as_str()));
    }
    if let Some(from) = filter.from {
        query = query.filter(AuditRecords::occurred_at().gte(from));
    }
    if let Some(to) = filter.to {
        query = query.filter(AuditRecords::occurred_at().lte(to));
    }
    if let Some(after) = after {
        query = query.filter(
            AuditRecords::occurred_at()
                .lt(after.occurred_at)
                .or(AuditRecords::occurred_at()
                    .eq(after.occurred_at)
                    .and(AuditRecords::audit_id().lt(after.audit_id))),
        );
    }
    fetch_all::<AuditRecordRow, _>(
        transaction,
        query
            .order_by(AuditRecords::occurred_at(), OrderDirection::Desc)
            .order_by(AuditRecords::audit_id(), OrderDirection::Desc)
            .limit(limit.max(1) as u64),
    )
    .await?
    .into_iter()
    .map(decode_record)
    .collect::<Result<Vec<_>, _>>()
    .map_err(Into::into)
}

async fn sweep_retention_in_transaction(
    transaction: &PostgresTransaction,
    sweep: AuditRetentionSweep,
) -> Result<AuditRetentionReport, PostgresPersistenceError> {
    let rows = fetch_all::<AuditRetentionStateRow, _>(
        transaction,
        retention_state_query()
            .filter(AuditRetentionStates::next_scan_at().lte(sweep.swept_at))
            .order_by(AuditRetentionStates::next_scan_at(), OrderDirection::Asc)
            .order_by(AuditRetentionStates::organization_id(), OrderDirection::Asc)
            .limit(sweep.organization_batch_size as u64)
            .for_update()
            .skip_locked(),
    )
    .await?;
    let states = rows
        .into_iter()
        .map(decode_retention_state)
        .collect::<Result<Vec<_>, _>>()?;
    let mut report = AuditRetentionReport::default();
    let mut remaining_records = sweep.record_batch_size;
    for state in states {
        if remaining_records == 0 {
            break;
        }
        report.inspected_organizations += 1;
        let boundary = state
            .records_available_from
            .map_or(sweep.cutoff, |current| current.max(sweep.cutoff));
        let candidates = select_from::<AuditRecords>()
            .select(AuditRecords::audit_id())
            .filter(AuditRecords::organization_id().eq(state.organization_id.as_uuid()))
            .filter(AuditRecords::occurred_at().lt(boundary))
            .order_by(AuditRecords::occurred_at(), OrderDirection::Asc)
            .order_by(AuditRecords::audit_id(), OrderDirection::Asc)
            .limit(remaining_records as u64);
        let deleted = execute(
            transaction,
            delete_from::<AuditRecords>()
                .filter(AuditRecords::organization_id().eq(state.organization_id.as_uuid()))
                .filter(AuditRecords::audit_id().in_subquery(candidates)),
        )
        .await?;
        if deleted > remaining_records as u64 {
            return Err(PostgresPersistenceError::Invariant(
                "audit retention deletion exceeded its record batch".into(),
            ));
        }
        let completed = fetch_optional::<Uuid, _>(
            transaction,
            select_from::<AuditRecords>()
                .select(AuditRecords::audit_id())
                .filter(AuditRecords::organization_id().eq(state.organization_id.as_uuid()))
                .filter(AuditRecords::occurred_at().lt(boundary))
                .limit(1),
        )
        .await?
        .is_none();
        let total_deleted_records = state
            .total_deleted_records
            .checked_add(deleted)
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "audit retention deleted-record count overflowed".into(),
                )
            })?;
        let version = state.version.checked_add(1).ok_or_else(|| {
            PostgresPersistenceError::Invariant("audit retention version overflowed".into())
        })?;
        let records_deleted_before = if completed {
            Some(boundary)
        } else {
            state.records_deleted_before
        };
        let last_completed_at = if completed {
            Some(sweep.swept_at)
        } else {
            state.last_completed_at
        };
        require_one_row(
            "audit retention state",
            execute(
                transaction,
                update_table::<AuditRetentionStates>()
                    .set(
                        AuditRetentionStates::records_available_from(),
                        Some(boundary),
                    )
                    .set(
                        AuditRetentionStates::records_deleted_before(),
                        records_deleted_before,
                    )
                    .set(
                        AuditRetentionStates::applied_policy_digest(),
                        Some(sweep.policy_digest.as_str().to_owned()),
                    )
                    .set(
                        AuditRetentionStates::total_deleted_records(),
                        total_deleted_records,
                    )
                    .set(AuditRetentionStates::last_swept_at(), Some(sweep.swept_at))
                    .set(AuditRetentionStates::last_completed_at(), last_completed_at)
                    .set(AuditRetentionStates::next_scan_at(), sweep.next_scan_at)
                    .set(AuditRetentionStates::version(), version)
                    .filter(
                        AuditRetentionStates::organization_id().eq(state.organization_id.as_uuid()),
                    )
                    .filter(AuditRetentionStates::version().eq(state.version)),
            )
            .await?,
        )?;
        report.deleted_records += deleted as usize;
        remaining_records -= deleted as usize;
        if completed {
            report.completed_organizations += 1;
        }
    }
    Ok(report)
}

async fn load_retention_state_for_share(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
) -> Result<Option<AuditRetentionState>, PostgresPersistenceError> {
    fetch_optional::<AuditRetentionStateRow, _>(
        transaction,
        retention_state_query()
            .filter(AuditRetentionStates::organization_id().eq(organization_id.as_uuid()))
            .for_share(),
    )
    .await?
    .map(decode_retention_state)
    .transpose()
    .map_err(Into::into)
}

async fn load_retention_state_for_update(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
) -> Result<Option<AuditRetentionState>, PostgresPersistenceError> {
    fetch_optional::<AuditRetentionStateRow, _>(
        transaction,
        retention_state_query()
            .filter(AuditRetentionStates::organization_id().eq(organization_id.as_uuid()))
            .for_update(),
    )
    .await?
    .map(decode_retention_state)
    .transpose()
    .map_err(Into::into)
}

fn retention_state_query(
) -> a3s_orm::query::SelectQuery<AuditRetentionStates, AuditRetentionStateRow> {
    select_from::<AuditRetentionStates>().select(AuditRetentionStateSelection)
}

fn decode_retention_state(
    row: AuditRetentionStateRow,
) -> Result<AuditRetentionState, RepositoryError> {
    let state = AuditRetentionState {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        records_available_from: row.records_available_from,
        records_deleted_before: row.records_deleted_before,
        applied_policy_digest: row
            .applied_policy_digest
            .map(Sha256Digest::parse)
            .transpose()
            .map_err(RepositoryError::Storage)?,
        total_deleted_records: row.total_deleted_records,
        last_swept_at: row.last_swept_at,
        last_completed_at: row.last_completed_at,
        next_scan_at: row.next_scan_at,
        version: row.version,
    };
    state.validate().map_err(RepositoryError::Storage)?;
    Ok(state)
}

fn decode_record(row: AuditRecordRow) -> Result<AuditRecord, RepositoryError> {
    let record = AuditRecord {
        id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        actor_principal_id: row.actor_principal_id.map(PrincipalId::from_uuid),
        action: row.action,
        aggregate_id: row.aggregate_id,
        occurred_at: row.occurred_at,
        request_id: row.request_id,
        project_id: row.project_id.map(ProjectId::from_uuid),
        environment_id: row.environment_id.map(EnvironmentId::from_uuid),
        attribution_profile_id: row
            .attribution_profile_id
            .map(ProjectAttributionProfileId::from_uuid),
        attribution_status: AuditAttributionStatus::parse(&row.attribution_status)
            .map_err(RepositoryError::Storage)?,
    };
    record.validate().map_err(RepositoryError::Storage)?;
    Ok(record)
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn decode_column<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}
