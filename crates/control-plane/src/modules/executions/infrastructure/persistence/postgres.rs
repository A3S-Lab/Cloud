use crate::infrastructure::{
    execute, fetch_optional, idempotency_replay, store_idempotency, store_outbox,
    transaction_error, PostgresPersistenceError,
};
use crate::modules::executions::domain::{
    validate_execution_transition, CreateExecution, Execution, ExecutionOutcome, ExecutionStatus,
    ExecutionTemplate, ExecutionWrite, IExecutionRepository, TransitionExecution,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, ExecutionId, IdempotencyRequest, NodeCommandId, NodeId, OperationId,
    OrganizationId, ProjectId, RepositoryError,
};
use a3s_orm::{
    sql_query, Database, DecodeError, FromRow, FromValue, PostgresDialect, PostgresExecutor, Row,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const SELECT_EXECUTIONS: &str = "select e.organization_id, e.project_id, e.environment_id, e.id, e.operation_id, e.template, e.template_digest, e.status, e.node_id, e.command_id, e.cleanup_command_id, e.runtime_spec_digest, e.outcome, e.aggregate_version, e.requested_at, e.updated_at, e.started_at, e.cancellation_requested_at, e.finished_at from executions e";

#[derive(Clone)]
pub struct PostgresExecutionRepository {
    executor: PostgresExecutor,
}

impl PostgresExecutionRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IExecutionRepository for PostgresExecutionRepository {
    async fn create(&self, request: CreateExecution) -> Result<ExecutionWrite, RepositoryError> {
        let execution = request
            .execution
            .restore()
            .map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<Execution>(transaction, &request.idempotency).await?
                    {
                        return Ok(ExecutionWrite {
                            execution: replay.value,
                            replayed: true,
                        });
                    }
                    insert_execution(transaction, &execution).await?;
                    store_outbox(transaction, &request.event).await?;
                    store_idempotency(transaction, &request.idempotency, &execution).await?;
                    Ok(ExecutionWrite {
                        execution,
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
        execution_id: ExecutionId,
    ) -> Result<Option<Execution>, RepositoryError> {
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                sql_query::<ExecutionRow>(SELECT_EXECUTIONS)
                    .append(" where e.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and e.id = ")
                    .bind(execution_id.as_uuid()),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .map(map_row)
            .transpose()
    }

    async fn list(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ExecutionRow>(SELECT_EXECUTIONS)
                    .append(" where e.organization_id = ")
                    .bind(organization_id.as_uuid())
                    .append(" and e.project_id = ")
                    .bind(project_id.as_uuid())
                    .append(" and e.environment_id = ")
                    .bind(environment_id.as_uuid())
                    .append(" order by e.requested_at desc, e.id desc limit ")
                    .bind(limit),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .collect()
    }

    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<Execution>, RepositoryError> {
        let idempotency = idempotency.clone();
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    Ok(idempotency_replay::<Execution>(transaction, &idempotency)
                        .await?
                        .map(|replay| replay.value))
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn request_cancellation(
        &self,
        request: TransitionExecution,
    ) -> Result<ExecutionWrite, RepositoryError> {
        let execution = request
            .execution
            .restore()
            .map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    if let Some(replay) =
                        idempotency_replay::<Execution>(transaction, &request.idempotency).await?
                    {
                        return Ok(ExecutionWrite {
                            execution: replay.value,
                            replayed: true,
                        });
                    }
                    let existing =
                        find_for_update(transaction, execution.organization_id, execution.id)
                            .await?;
                    validate_execution_transition(&existing, &execution, request.expected_version)
                        .map_err(PostgresPersistenceError::Repository)?;
                    let execution =
                        persist_execution(transaction, &execution, request.expected_version)
                            .await?;
                    store_outbox(transaction, &request.event).await?;
                    store_idempotency(transaction, &request.idempotency, &execution).await?;
                    Ok(ExecutionWrite {
                        execution,
                        replayed: false,
                    })
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<Execution>, RepositoryError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(
                sql_query::<ExecutionRow>(SELECT_EXECUTIONS)
                    .append(
                        " left join operation_requests o on o.operation_id = e.operation_id where o.operation_id is null and e.status not in ('succeeded', 'failed', 'cancelled') order by e.requested_at asc, e.id asc limit ",
                    )
                    .bind(limit),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows
            .into_iter()
            .map(map_row)
            .collect()
    }

    async fn save(
        &self,
        execution: Execution,
        expected_version: u64,
    ) -> Result<Execution, RepositoryError> {
        let execution = execution.restore().map_err(RepositoryError::Storage)?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let existing =
                        find_for_update(transaction, execution.organization_id, execution.id)
                            .await?;
                    validate_execution_transition(&existing, &execution, expected_version)
                        .map_err(PostgresPersistenceError::Repository)?;
                    persist_execution(transaction, &execution, expected_version).await
                })
            })
            .await
            .map_err(transaction_error)
    }
}

async fn insert_execution(
    transaction: &a3s_orm::PostgresTransaction,
    execution: &Execution,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into executions (organization_id, project_id, environment_id, id, operation_id, template, template_digest, status, aggregate_version, requested_at, updated_at) values (",
        )
        .bind(execution.organization_id.as_uuid())
        .append(", ")
        .bind(execution.project_id.as_uuid())
        .append(", ")
        .bind(execution.environment_id.as_uuid())
        .append(", ")
        .bind(execution.id.as_uuid())
        .append(", ")
        .bind(execution.operation_id.as_uuid())
        .append(", ")
        .bind(serde_json::to_value(&execution.template)?)
        .append(", ")
        .bind(execution.template_digest.as_str())
        .append(", ")
        .bind(execution.status.as_str())
        .append(", ")
        .bind(execution.aggregate_version)
        .append(", ")
        .bind(execution.requested_at)
        .append(", ")
        .bind(execution.updated_at)
        .append(")"),
    )
    .await?;
    if rows != 1 {
        return Err(PostgresPersistenceError::Invariant(format!(
            "creating execution affected {rows} rows"
        )));
    }
    Ok(())
}

async fn find_for_update(
    transaction: &a3s_orm::PostgresTransaction,
    organization_id: OrganizationId,
    execution_id: ExecutionId,
) -> Result<Execution, PostgresPersistenceError> {
    let row = fetch_optional::<ExecutionRow, _>(
        transaction,
        sql_query::<ExecutionRow>(SELECT_EXECUTIONS)
            .append(" where e.organization_id = ")
            .bind(organization_id.as_uuid())
            .append(" and e.id = ")
            .bind(execution_id.as_uuid())
            .append(" for update"),
    )
    .await?
    .ok_or(PostgresPersistenceError::Repository(
        RepositoryError::NotFound,
    ))?;
    map_row(row).map_err(PostgresPersistenceError::Repository)
}

async fn persist_execution(
    transaction: &a3s_orm::PostgresTransaction,
    execution: &Execution,
    expected_version: u64,
) -> Result<Execution, PostgresPersistenceError> {
    let outcome = execution
        .outcome
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let rows = execute(
        transaction,
        sql_query::<()>("update executions set status = ")
            .bind(execution.status.as_str())
            .append(", node_id = ")
            .bind(execution.node_id.map(NodeId::as_uuid))
            .append(", command_id = ")
            .bind(execution.command_id.map(NodeCommandId::as_uuid))
            .append(", cleanup_command_id = ")
            .bind(execution.cleanup_command_id.map(NodeCommandId::as_uuid))
            .append(", runtime_spec_digest = ")
            .bind(execution.runtime_spec_digest.as_deref())
            .append(", outcome = ")
            .bind(outcome)
            .append(", aggregate_version = ")
            .bind(execution.aggregate_version)
            .append(", updated_at = ")
            .bind(execution.updated_at)
            .append(", started_at = ")
            .bind(execution.started_at)
            .append(", cancellation_requested_at = ")
            .bind(execution.cancellation_requested_at)
            .append(", finished_at = ")
            .bind(execution.finished_at)
            .append(" where organization_id = ")
            .bind(execution.organization_id.as_uuid())
            .append(" and id = ")
            .bind(execution.id.as_uuid())
            .append(" and aggregate_version = ")
            .bind(expected_version),
    )
    .await?;
    match rows {
        1 => {}
        0 => {
            return Err(PostgresPersistenceError::Repository(
                RepositoryError::Conflict("execution changed concurrently".into()),
            ))
        }
        rows => {
            return Err(PostgresPersistenceError::Invariant(format!(
                "updating execution affected {rows} rows"
            )))
        }
    }
    let row = fetch_optional::<ExecutionRow, _>(
        transaction,
        sql_query::<ExecutionRow>(SELECT_EXECUTIONS)
            .append(" where e.organization_id = ")
            .bind(execution.organization_id.as_uuid())
            .append(" and e.id = ")
            .bind(execution.id.as_uuid()),
    )
    .await?
    .ok_or_else(|| {
        PostgresPersistenceError::Invariant("updated execution could not be reloaded".into())
    })?;
    map_row(row).map_err(PostgresPersistenceError::Repository)
}

struct ExecutionRow {
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    id: Uuid,
    operation_id: Uuid,
    template: Value,
    template_digest: String,
    status: String,
    node_id: Option<Uuid>,
    command_id: Option<Uuid>,
    cleanup_command_id: Option<Uuid>,
    runtime_spec_digest: Option<String>,
    outcome: Option<Value>,
    aggregate_version: u64,
    requested_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    cancellation_requested_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl FromRow for ExecutionRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            project_id: decode(row, 1)?,
            environment_id: decode(row, 2)?,
            id: decode(row, 3)?,
            operation_id: decode(row, 4)?,
            template: decode(row, 5)?,
            template_digest: decode(row, 6)?,
            status: decode(row, 7)?,
            node_id: decode(row, 8)?,
            command_id: decode(row, 9)?,
            cleanup_command_id: decode(row, 10)?,
            runtime_spec_digest: decode(row, 11)?,
            outcome: decode(row, 12)?,
            aggregate_version: decode(row, 13)?,
            requested_at: decode(row, 14)?,
            updated_at: decode(row, 15)?,
            started_at: decode(row, 16)?,
            cancellation_requested_at: decode(row, 17)?,
            finished_at: decode(row, 18)?,
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

fn map_row(row: ExecutionRow) -> Result<Execution, RepositoryError> {
    let template = serde_json::from_value::<ExecutionTemplate>(row.template)
        .map_err(|error| corrupt(format!("stored execution template is invalid: {error}")))?;
    let outcome = row
        .outcome
        .map(serde_json::from_value::<ExecutionOutcome>)
        .transpose()
        .map_err(|error| corrupt(format!("stored execution outcome is invalid: {error}")))?;
    Execution {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        project_id: ProjectId::from_uuid(row.project_id),
        environment_id: EnvironmentId::from_uuid(row.environment_id),
        id: ExecutionId::from_uuid(row.id),
        operation_id: OperationId::from_uuid(row.operation_id),
        template,
        template_digest: row.template_digest,
        status: ExecutionStatus::parse(&row.status)
            .map_err(|error| corrupt(format!("stored execution status is invalid: {error}")))?,
        node_id: row.node_id.map(NodeId::from_uuid),
        command_id: row.command_id.map(NodeCommandId::from_uuid),
        cleanup_command_id: row.cleanup_command_id.map(NodeCommandId::from_uuid),
        runtime_spec_digest: row.runtime_spec_digest,
        outcome,
        aggregate_version: row.aggregate_version,
        requested_at: row.requested_at,
        updated_at: row.updated_at,
        started_at: row.started_at,
        cancellation_requested_at: row.cancellation_requested_at,
        finished_at: row.finished_at,
    }
    .restore()
    .map_err(|error| corrupt(format!("stored execution is invalid: {error}")))
}

fn corrupt(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(message.into())
}
