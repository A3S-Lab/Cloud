use super::postgres_schema::{IdempotencyRecords, MigrationRecords, OutboxEvents};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, IdempotentWrite, RepositoryError};
use a3s_boot::HealthIndicatorResult;
use a3s_cloud_contracts::DomainEventEnvelope;
use a3s_orm::migration::MigrationRunError;
use a3s_orm::{
    insert_into, select_from, Database, DecodeError, Executor, FromRow, Migration, Migrator,
    PostgresDialect, PostgresError, PostgresExecutor, PostgresMigrationError, PostgresTransaction,
    PostgresTransactionError, Query,
};
use chrono::Utc;
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
        Migration::new(
            "010",
            "Gateway snapshot commands",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/010_gateway_snapshot_commands.sql"
            )),
        ),
        Migration::new(
            "011",
            "Edge route publications",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/011_edge_routes.sql"
            )),
        ),
        Migration::new(
            "012",
            "Edge domain ownership and TLS certificates",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/012_edge_tls.sql"
            )),
        ),
        Migration::new(
            "013",
            "encrypted Secret resources",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/013_secrets.sql"
            )),
        ),
        Migration::new(
            "014",
            "durable log retention tombstones",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/014_log_retention.sql"
            )),
        ),
        Migration::new(
            "015",
            "bounded log tombstone compaction",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/015_log_tombstone_compaction.sql"
            )),
        ),
        Migration::new(
            "016",
            "durable provider log gaps",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/016_provider_log_gaps.sql"
            )),
        ),
        Migration::new(
            "017",
            "Secret rotation workload restarts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/017_secret_rotation_restarts.sql"
            )),
        ),
        Migration::new(
            "018",
            "Gateway route cutovers",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/018_gateway_route_cutovers.sql"
            )),
        ),
        Migration::new(
            "019",
            "deployment retirement",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/019_deployment_retirement.sql"
            )),
        ),
        Migration::new(
            "020",
            "Gateway certificate convergence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/020_gateway_certificate_convergence.sql"
            )),
        ),
        Migration::new(
            "021",
            "external source revisions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/021_external_source_revisions.sql"
            )),
        ),
        Migration::new(
            "022",
            "source webhook inbox",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/022_source_webhook_inbox.sql"
            )),
        ),
        Migration::new(
            "023",
            "GitHub source connections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/023_github_source_connections.sql"
            )),
        ),
        Migration::new(
            "024",
            "GitHub repository subscriptions",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/024_github_repository_subscriptions.sql"
            )),
        ),
        Migration::new(
            "025",
            "GitHub connection lifecycle",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/025_github_connection_lifecycle.sql"
            )),
        ),
        Migration::new(
            "026",
            "durable source build runs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/026_build_runs.sql"
            )),
        ),
        Migration::new(
            "027",
            "durable OCI build publications",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/027_build_publications.sql"
            )),
        ),
        Migration::new(
            "028",
            "external build workload handoff",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/028_external_build_workload_handoff.sql"
            )),
        ),
        Migration::new(
            "029",
            "GitHub provider authority",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/029_github_provider_authority.sql"
            )),
        ),
        Migration::new(
            "030",
            "build run attempts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/030_build_run_attempts.sql"
            )),
        ),
        Migration::new(
            "031",
            "verified build evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/031_build_evidence.sql"
            )),
        ),
        Migration::new(
            "032",
            "trusted build cache",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/032_build_cache_trust.sql"
            )),
        ),
        Migration::new(
            "033",
            "managed Gateway snapshot validity",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/033_gateway_snapshot_validity.sql"
            )),
        ),
        Migration::new(
            "034",
            "managed Gateway snapshot renewal",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/034_gateway_snapshot_renewal.sql"
            )),
        ),
        Migration::new(
            "035",
            "generation-bound Gateway route targets",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/035_route_target_generation.sql"
            )),
        ),
        Migration::new(
            "036",
            "logical Gateway scopes",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/036_logical_gateway_scopes.sql"
            )),
        ),
        Migration::new(
            "037",
            "Gateway management protocol evidence",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/037_gateway_management_protocol.sql"
            )),
        ),
        Migration::new(
            "038",
            "replicated Gateway scope membership",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/038_gateway_scope_membership.sql"
            )),
        ),
        Migration::new(
            "039",
            "per-replica Gateway rollouts",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/039_gateway_replica_rollouts.sql"
            )),
        ),
        Migration::new(
            "040",
            "managed Workload replica foundation",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/040_workload_replica_foundation.sql"
            )),
        ),
        Migration::new(
            "041",
            "fenced hard resource claims",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/041_hard_resource_claims.sql"
            )),
        ),
        Migration::new(
            "042",
            "node resource inventories",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/042_node_resource_inventories.sql"
            )),
        ),
        Migration::new(
            "043",
            "shared resource capacity accounting",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/043_shared_resource_capacity.sql"
            )),
        ),
        Migration::new(
            "044",
            "Agent resource Claim commands",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/044_resource_claim_commands.sql"
            )),
        ),
        Migration::new(
            "045",
            "Gateway Route rollout projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/045_gateway_route_rollout_projections.sql"
            )),
        ),
        Migration::new(
            "046",
            "Gateway snapshot observation commands",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/046_gateway_snapshot_observation_commands.sql"
            )),
        ),
        Migration::new(
            "047",
            "Gateway replica physical-state recovery",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/047_gateway_replica_recovery.sql"
            )),
        ),
        Migration::new(
            "048",
            "Gateway rollout exact rollback",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/048_gateway_rollout_rollbacks.sql"
            )),
        ),
        Migration::new(
            "049",
            "Gateway certificate convergence unavailability",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/049_gateway_certificate_convergence_unavailable.sql"
            )),
        ),
        Migration::new(
            "050",
            "tenant-authorized search projections",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../migrations/050_authorized_search_projections.sql"
            )),
        ),
    ]
}

async fn verify_postgres(executor: &PostgresExecutor) -> Result<(), PostgresBootstrapError> {
    Database::new(PostgresDialect, executor.clone())
        .fetch_one_as(readiness_query())
        .await
        .map(|_| ())
        .map_err(|error| PostgresBootstrapError::Readiness(error.to_string()))
}

pub async fn postgres_health(executor: PostgresExecutor) -> HealthIndicatorResult {
    match Database::new(PostgresDialect, executor)
        .fetch_one_as(readiness_query())
        .await
    {
        Ok(_) => HealthIndicatorResult::up(),
        Err(error) => HealthIndicatorResult::down().with_detail_value("error", error.to_string()),
    }
}

fn readiness_query() -> a3s_orm::query::SelectQuery<MigrationRecords, String> {
    select_from::<MigrationRecords>()
        .select(MigrationRecords::version())
        .limit(1)
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
    transaction
        .advisory_xact_lock(idempotency.scope.as_str(), idempotency.key.as_str())
        .await?;
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
        select_from::<IdempotencyRecords>()
            .select((
                IdempotencyRecords::request_digest(),
                IdempotencyRecords::response(),
            ))
            .filter(IdempotencyRecords::scope_key().eq(idempotency.scope.as_str()))
            .filter(IdempotencyRecords::idempotency_key().eq(idempotency.key.as_str())),
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
        insert_into::<IdempotencyRecords>()
            .value(IdempotencyRecords::scope_key(), idempotency.scope.as_str())
            .value(
                IdempotencyRecords::idempotency_key(),
                idempotency.key.as_str(),
            )
            .value(
                IdempotencyRecords::request_digest(),
                idempotency.request_digest.as_str(),
            )
            .value(
                IdempotencyRecords::response(),
                serde_json::to_value(response)?,
            )
            .value(IdempotencyRecords::created_at(), Utc::now()),
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
        insert_into::<OutboxEvents>()
            .value(OutboxEvents::event_id(), event.event_id)
            .value(OutboxEvents::event_key(), event.event_key.as_str())
            .value(OutboxEvents::schema_version(), event.schema_version)
            .value(OutboxEvents::organization_id(), event.organization_id)
            .value(OutboxEvents::aggregate_id(), event.aggregate_id)
            .value(OutboxEvents::aggregate_version(), event.aggregate_version)
            .value(OutboxEvents::occurred_at(), event.occurred_at)
            .value(OutboxEvents::correlation_id(), event.correlation_id)
            .value(OutboxEvents::causation_id(), event.causation_id)
            .value(OutboxEvents::payload(), event.payload.clone()),
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
