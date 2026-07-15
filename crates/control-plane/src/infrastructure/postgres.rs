use crate::modules::shared_kernel::domain::{IdempotencyRequest, IdempotentWrite, RepositoryError};
use a3s_boot::HealthIndicatorResult;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::migration::MigrationRunError;
use a3s_orm::{
    sql_query, Database, DecodeError, Executor, FromRow, Migration, Migrator, PostgresDialect,
    PostgresError, PostgresExecutor, PostgresMigrationError, PostgresTransaction,
    PostgresTransactionError, Query,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum PostgresBootstrapError {
    #[error("could not configure PostgreSQL: {0}")]
    Connect(#[from] PostgresError),
    #[error("could not migrate PostgreSQL: {0}")]
    Migrate(#[from] MigrationRunError<PostgresMigrationError>),
    #[error("PostgreSQL did not become ready: {0}")]
    Readiness(String),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PostgresPersistenceError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error("could not build PostgreSQL query: {0}")]
    Query(#[from] a3s_orm::Error),
    #[error("PostgreSQL query failed: {0}")]
    Database(#[from] PostgresError),
    #[error("could not decode PostgreSQL row: {0}")]
    Decode(#[from] DecodeError),
    #[error("could not serialize persisted response: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("PostgreSQL query returned {actual} rows where at most one was expected")]
    Cardinality { actual: usize },
    #[error("PostgreSQL persistence invariant failed: {0}")]
    Invariant(String),
}

impl PostgresPersistenceError {
    fn into_repository(self) -> RepositoryError {
        match self {
            Self::Repository(error) => error,
            error => RepositoryError::Storage(error.to_string()),
        }
    }
}

pub async fn connect_and_migrate(
    url: &str,
    max_connections: usize,
) -> Result<PostgresExecutor, PostgresBootstrapError> {
    let executor = PostgresExecutor::connect_no_tls(url, max_connections)?;
    Migrator::new(executor.clone())
        .run(cloud_migrations())
        .await?;
    verify_postgres(&executor).await?;
    Ok(executor)
}

fn cloud_migrations() -> Vec<Migration> {
    vec![
        Migration::new(
            "001",
            "cloud foundation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/001_foundation.sql"
            )),
        ),
        Migration::new(
            "002",
            "flow operations",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/002_flow_operations.sql"
            )),
        ),
        Migration::new(
            "003",
            "outbox leases",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/003_outbox_leases.sql"
            )),
        ),
        Migration::new(
            "004",
            "API tokens",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/004_api_tokens.sql"
            )),
        ),
        Migration::new(
            "005",
            "fleet node control",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/005_fleet.sql"
            )),
        ),
        Migration::new(
            "006",
            "workloads and deployments",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/006_workloads.sql"
            )),
        ),
        Migration::new(
            "007",
            "deployment cancellation cleanup",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/007_deployment_cleanup.sql"
            )),
        ),
        Migration::new(
            "008",
            "workload revision resolution",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/008_workload_revision_resolution.sql"
            )),
        ),
        Migration::new(
            "009",
            "same-generation Runtime apply recovery",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/009_runtime_apply_recovery.sql"
            )),
        ),
    ]
}

async fn verify_postgres(executor: &PostgresExecutor) -> Result<(), PostgresBootstrapError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(sql_query::<i32>("select 1"))
        .await
        .map(|_| ())
        .map_err(|error| PostgresBootstrapError::Readiness(error.to_string()))
}

pub async fn postgres_health(executor: PostgresExecutor) -> HealthIndicatorResult {
    match Database::new(PostgresDialect, executor)
        .fetch_one_as(sql_query::<i32>("select 1"))
        .await
    {
        Ok(1) => HealthIndicatorResult::up(),
        Ok(_) => HealthIndicatorResult::down().with_detail_value("error", "unexpected response"),
        Err(error) => HealthIndicatorResult::down().with_detail_value("error", error.to_string()),
    }
}

pub(crate) async fn execute<Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<u64, PostgresPersistenceError>
where
    Q: Query,
{
    let query = query.compile(&PostgresDialect)?;
    Ok(transaction.execute(&query).await?.rows_affected)
}

pub(crate) async fn fetch_optional<O, Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<Option<O>, PostgresPersistenceError>
where
    O: FromRow,
    Q: Query<Output = O>,
{
    let rows = fetch_all(transaction, query).await?;
    if rows.len() > 1 {
        return Err(PostgresPersistenceError::Cardinality { actual: rows.len() });
    }
    Ok(rows.into_iter().next())
}

pub(crate) async fn fetch_all<O, Q>(
    transaction: &PostgresTransaction,
    query: Q,
) -> Result<Vec<O>, PostgresPersistenceError>
where
    O: FromRow,
    Q: Query<Output = O>,
{
    let query = query.compile(&PostgresDialect)?;
    transaction
        .fetch_all(&query)
        .await?
        .rows
        .iter()
        .map(O::from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub(crate) async fn lock_idempotency_key(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<(), PostgresPersistenceError> {
    let locked = fetch_optional::<i32, _>(
        transaction,
        sql_query::<i32>("select 1 from (select pg_advisory_xact_lock(hashtext(")
            .bind(idempotency.scope.as_str())
            .append("), hashtext(")
            .bind(idempotency.key.as_str())
            .append("))) as locked"),
    )
    .await?;
    if locked != Some(1) {
        return Err(PostgresPersistenceError::Invariant(
            "idempotency advisory lock did not return a row".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn idempotency_replay<T>(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
) -> Result<Option<IdempotentWrite<T>>, PostgresPersistenceError>
where
    T: DeserializeOwned,
{
    lock_idempotency_key(transaction, idempotency).await?;
    let existing = fetch_optional::<(String, serde_json::Value), _>(
        transaction,
        sql_query::<(String, serde_json::Value)>(
            "select request_digest, response from idempotency_records where scope_key = ",
        )
        .bind(idempotency.scope.as_str())
        .append(" and idempotency_key = ")
        .bind(idempotency.key.as_str())
        .append(" for update"),
    )
    .await?;
    let Some((request_digest, response)) = existing else {
        return Ok(None);
    };
    if request_digest != idempotency.request_digest {
        return Err(RepositoryError::IdempotencyConflict.into());
    }
    Ok(Some(IdempotentWrite {
        value: serde_json::from_value(response)?,
        replayed: true,
    }))
}

pub(crate) async fn store_idempotency<T>(
    transaction: &PostgresTransaction,
    idempotency: &IdempotencyRequest,
    response: &T,
) -> Result<(), PostgresPersistenceError>
where
    T: Serialize,
{
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into idempotency_records (scope_key, idempotency_key, request_digest, response, created_at) values (",
        )
        .bind(idempotency.scope.as_str())
        .append(", ")
        .bind(idempotency.key.as_str())
        .append(", ")
        .bind(idempotency.request_digest.as_str())
        .append(", ")
        .bind(serde_json::to_value(response)?)
        .append(", now())"),
    )
    .await?;
    require_one_row("idempotency record", rows)
}

pub(crate) async fn store_outbox(
    transaction: &PostgresTransaction,
    event: &DomainEventEnvelope,
) -> Result<(), PostgresPersistenceError> {
    let rows = execute(
        transaction,
        sql_query::<()>(
            "insert into outbox_events (event_id, event_key, schema_version, organization_id, aggregate_id, aggregate_version, occurred_at, correlation_id, causation_id, payload) values (",
        )
        .bind(event.event_id)
        .append(", ")
        .bind(event.event_key.as_str())
        .append(", ")
        .bind(event.schema_version)
        .append(", ")
        .bind(event.organization_id)
        .append(", ")
        .bind(event.aggregate_id)
        .append(", ")
        .bind(event.aggregate_version)
        .append(", ")
        .bind(event.occurred_at)
        .append(", ")
        .bind(event.correlation_id)
        .append(", ")
        .bind(event.causation_id)
        .append(", ")
        .bind(event.payload.clone())
        .append(")"),
    )
    .await?;
    require_one_row("outbox event", rows)
}

pub(crate) fn require_one_row(
    resource: &str,
    rows_affected: u64,
) -> Result<(), PostgresPersistenceError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Invariant(format!(
            "writing {resource} affected {rows_affected} rows"
        )))
    }
}

pub(crate) fn is_unique_violation(error: &PostgresPersistenceError) -> bool {
    database_error_code(error) == Some("23505")
}

pub(crate) fn is_foreign_key_violation(error: &PostgresPersistenceError) -> bool {
    database_error_code(error) == Some("23503")
}

fn database_error_code(error: &PostgresPersistenceError) -> Option<&str> {
    let PostgresPersistenceError::Database(PostgresError::Database(error)) = error else {
        return None;
    };
    error.code().map(|code| code.code())
}

pub(crate) fn transaction_error(
    error: PostgresTransactionError<PostgresPersistenceError>,
) -> RepositoryError {
    match error {
        PostgresTransactionError::Operation(error) => error.into_repository(),
        PostgresTransactionError::Begin(error) => {
            RepositoryError::Storage(format!("could not begin PostgreSQL transaction: {error}"))
        }
        PostgresTransactionError::Commit(error) => {
            RepositoryError::Storage(format!("could not commit PostgreSQL transaction: {error}"))
        }
        PostgresTransactionError::OperationAndRollback {
            operation,
            rollback,
        } => RepositoryError::Storage(format!(
            "PostgreSQL operation failed ({operation}) and rollback failed ({rollback})"
        )),
    }
}
