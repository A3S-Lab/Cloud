use crate::infrastructure::{
    execute, fetch_optional, is_foreign_key_violation, is_unique_violation, require_one_row,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::automations::domain::{
    AutomationScheduleLease, AutomationScheduleState, AutomationScheduleStateKey,
    CommitAutomationScheduleCursor, IAutomationScheduleStateRepository,
    ReserveAutomationScheduleLease,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, Sha256Digest,
};
use a3s_orm::{
    sql_query, DecodeError, FromRow, FromValue, PostgresExecutor, PostgresTransaction, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const SELECT_STATE: &str = "select organization_id, project_id, environment_id, automation_id, revision_id, revision_digest, cursor_at, lease_generation, lease_owner_id, lease_id, reserved_at, lease_expires_at, created_at, updated_at from automation_schedule_states";

#[derive(Clone)]
pub struct PostgresAutomationScheduleStateRepository {
    executor: PostgresExecutor,
}

impl PostgresAutomationScheduleStateRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IAutomationScheduleStateRepository for PostgresAutomationScheduleStateRepository {
    async fn create(
        &self,
        state: AutomationScheduleState,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    state
                        .validate_for_creation()
                        .map_err(PostgresPersistenceError::Invariant)?;
                    insert_state(transaction, &state).await
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        key: AutomationScheduleStateKey,
    ) -> Result<Option<AutomationScheduleState>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move { load_state(transaction, key, false).await })
            })
            .await
            .map_err(transaction_error)
    }

    async fn reserve(
        &self,
        request: ReserveAutomationScheduleLease,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut state = load_state(transaction, request.key, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    state.reserve_lease(
                        request.owner_id,
                        request.lease_id,
                        request.reserved_at,
                        request.lease_expires_at,
                    )?;
                    persist_state(transaction, &state).await?;
                    Ok(state)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn commit_cursor(
        &self,
        request: CommitAutomationScheduleCursor,
    ) -> Result<AutomationScheduleState, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let mut state = load_state(transaction, request.key, true)
                        .await?
                        .ok_or(RepositoryError::NotFound)?;
                    state.commit_cursor(
                        request.owner_id,
                        request.lease_id,
                        request.lease_generation,
                        request.evaluated_through,
                        request.committed_at,
                    )?;
                    persist_state(transaction, &state).await?;
                    Ok(state)
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_state(
    transaction: &PostgresTransaction,
    state: &AutomationScheduleState,
) -> Result<AutomationScheduleState, PostgresPersistenceError> {
    let key = state.key();
    let rows = execute(
        transaction,
        sql_query::<()>("insert into automation_schedule_states (organization_id, project_id, environment_id, automation_id, revision_id, revision_digest, cursor_at, lease_generation, lease_owner_id, lease_id, reserved_at, lease_expires_at, created_at, updated_at) values (")
            .bind(key.organization_id.as_uuid())
            .append(", ")
            .bind(key.project_id.as_uuid())
            .append(", ")
            .bind(key.environment_id.as_uuid())
            .append(", ")
            .bind(key.automation_id)
            .append(", ")
            .bind(state.revision_id())
            .append(", ")
            .bind(state.revision_digest().as_str())
            .append(", ")
            .bind(state.cursor_at())
            .append(", ")
            .bind(state.lease_generation())
            .append(", null, null, null, null, ")
            .bind(state.created_at())
            .append(", ")
            .bind(state.updated_at())
            .append(")"),
    )
    .await;
    match rows {
        Ok(rows) => {
            require_one_row("Automation schedule state", rows)?;
            Ok(state.clone())
        }
        Err(error) if is_unique_violation(&error) => Err(PostgresPersistenceError::Repository(
            RepositoryError::Conflict("Automation schedule state already exists".into()),
        )),
        Err(error) if is_foreign_key_violation(&error) => Err(
            PostgresPersistenceError::Repository(RepositoryError::NotFound),
        ),
        Err(error) => Err(error),
    }
}

async fn load_state(
    transaction: &PostgresTransaction,
    key: AutomationScheduleStateKey,
    for_update: bool,
) -> Result<Option<AutomationScheduleState>, PostgresPersistenceError> {
    key.validate()
        .map_err(PostgresPersistenceError::Invariant)?;
    let mut query = sql_query::<AutomationScheduleStateRow>(SELECT_STATE)
        .append(" where organization_id = ")
        .bind(key.organization_id.as_uuid())
        .append(" and project_id = ")
        .bind(key.project_id.as_uuid())
        .append(" and environment_id = ")
        .bind(key.environment_id.as_uuid())
        .append(" and automation_id = ")
        .bind(key.automation_id);
    if for_update {
        query = query.append(" for update");
    }
    fetch_optional(transaction, query)
        .await?
        .map(decode_state)
        .transpose()
}

async fn persist_state(
    transaction: &PostgresTransaction,
    state: &AutomationScheduleState,
) -> Result<(), PostgresPersistenceError> {
    let key = state.key();
    let lease = state.lease();
    let rows = execute(
        transaction,
        sql_query::<()>("update automation_schedule_states set revision_id = ")
            .bind(state.revision_id())
            .append(", revision_digest = ")
            .bind(state.revision_digest().as_str())
            .append(", cursor_at = ")
            .bind(state.cursor_at())
            .append(", lease_generation = ")
            .bind(state.lease_generation())
            .append(", lease_owner_id = ")
            .bind(lease.map(AutomationScheduleLease::owner_id))
            .append(", lease_id = ")
            .bind(lease.map(AutomationScheduleLease::lease_id))
            .append(", reserved_at = ")
            .bind(lease.map(AutomationScheduleLease::reserved_at))
            .append(", lease_expires_at = ")
            .bind(lease.map(AutomationScheduleLease::lease_expires_at))
            .append(", created_at = ")
            .bind(state.created_at())
            .append(", updated_at = ")
            .bind(state.updated_at())
            .append(" where organization_id = ")
            .bind(key.organization_id.as_uuid())
            .append(" and project_id = ")
            .bind(key.project_id.as_uuid())
            .append(" and environment_id = ")
            .bind(key.environment_id.as_uuid())
            .append(" and automation_id = ")
            .bind(key.automation_id),
    )
    .await?;
    require_one_row("Automation schedule state update", rows)
}

struct AutomationScheduleStateRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    automation_id: Uuid,
    revision_id: Uuid,
    revision_digest: String,
    cursor_at: DateTime<Utc>,
    lease_generation: i64,
    lease_owner_id: Option<Uuid>,
    lease_id: Option<Uuid>,
    reserved_at: Option<DateTime<Utc>>,
    lease_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl FromRow for AutomationScheduleStateRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            automation_id: decode(row, 3)?,
            revision_id: decode(row, 4)?,
            revision_digest: decode(row, 5)?,
            cursor_at: decode(row, 6)?,
            lease_generation: decode(row, 7)?,
            lease_owner_id: decode(row, 8)?,
            lease_id: decode(row, 9)?,
            reserved_at: decode(row, 10)?,
            lease_expires_at: decode(row, 11)?,
            created_at: decode(row, 12)?,
            updated_at: decode(row, 13)?,
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

fn decode_state(
    row: AutomationScheduleStateRow,
) -> Result<AutomationScheduleState, PostgresPersistenceError> {
    let lease = match (
        row.lease_owner_id,
        row.lease_id,
        row.reserved_at,
        row.lease_expires_at,
    ) {
        (None, None, None, None) => None,
        (Some(owner_id), Some(lease_id), Some(reserved_at), Some(lease_expires_at)) => Some(
            AutomationScheduleLease::new(owner_id, lease_id, reserved_at, lease_expires_at)
                .map_err(PostgresPersistenceError::Invariant)?,
        ),
        _ => {
            return Err(PostgresPersistenceError::Invariant(
                "Automation schedule lease columns are partially populated".into(),
            ))
        }
    };
    let generation = u64::try_from(row.lease_generation).map_err(|_| {
        PostgresPersistenceError::Invariant(
            "Automation schedule lease generation is negative".into(),
        )
    })?;
    AutomationScheduleState::restore(
        AutomationScheduleStateKey::new(
            OrganizationId::from_uuid(row.organization_id),
            ProjectId::from_uuid(row.project_id),
            EnvironmentId::from_uuid(row.environment_id),
            row.automation_id,
        ),
        row.revision_id,
        Sha256Digest::parse(row.revision_digest).map_err(PostgresPersistenceError::Invariant)?,
        row.cursor_at,
        generation,
        lease,
        row.created_at,
        row.updated_at,
    )
    .map_err(PostgresPersistenceError::Invariant)
}
