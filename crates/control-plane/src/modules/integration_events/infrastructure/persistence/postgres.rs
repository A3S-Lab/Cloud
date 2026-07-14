use crate::infrastructure::{fetch_all, transaction_error, PostgresPersistenceError};
use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::integration_events::domain::repositories::IOutboxRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_orm::{sql_query, Database, PostgresDialect, PostgresExecutor};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresOutboxRepository {
    executor: PostgresExecutor,
}

impl PostgresOutboxRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl IOutboxRepository for PostgresOutboxRepository {
    async fn claim(
        &self,
        owner: Uuid,
        limit: usize,
        lease_duration: Duration,
    ) -> Result<Vec<OutboxMessage>, RepositoryError> {
        let lease_millis = u64::try_from(lease_duration.as_millis()).unwrap_or(u64::MAX);
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<serde_json::Value, _>(
                        transaction,
                        sql_query::<serde_json::Value>(
                            "with candidates as (select event_id from outbox_events where published_at is null and next_attempt_at <= now() and (leased_until is null or leased_until <= now()) order by next_attempt_at asc, occurred_at asc, event_id asc for update skip locked limit ",
                        )
                        .bind(limit.max(1))
                        .append(") update outbox_events e set lease_owner = ")
                        .bind(owner)
                        .append(", leased_until = now() + (")
                        .bind(lease_millis)
                        .append("::bigint * interval '1 millisecond'), delivery_attempts = e.delivery_attempts + 1 from candidates c where e.event_id = c.event_id returning jsonb_build_object('event_id', e.event_id, 'event_key', e.event_key, 'schema_version', e.schema_version, 'organization_id', e.organization_id, 'aggregate_id', e.aggregate_id, 'aggregate_version', e.aggregate_version, 'occurred_at', e.occurred_at, 'correlation_id', e.correlation_id, 'causation_id', e.causation_id, 'payload', e.payload, 'delivery_attempts', e.delivery_attempts)"),
                    )
                    .await?;
                    rows.into_iter()
                        .map(serde_json::from_value)
                        .collect::<Result<Vec<OutboxMessage>, _>>()
                        .map_err(PostgresPersistenceError::from)
                })
            })
            .await
            .map_err(transaction_error)
    }

    async fn mark_published(
        &self,
        event_id: Uuid,
        owner: Uuid,
        published_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        let result = Database::new(PostgresDialect, self.executor.clone())
            .execute(
                sql_query::<()>(
                    "update outbox_events set published_at = ",
                )
                .bind(published_at)
                .append(", lease_owner = null, leased_until = null, last_error = null where event_id = ")
                .bind(event_id)
                .append(" and lease_owner = ")
                .bind(owner)
                .append(" and published_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        require_claimed("publish", result.rows_affected)
    }

    async fn mark_failed(
        &self,
        event_id: Uuid,
        owner: Uuid,
        error: &str,
        retry_after: Duration,
    ) -> Result<(), RepositoryError> {
        let retry_millis = u64::try_from(retry_after.as_millis()).unwrap_or(u64::MAX);
        let error = error.chars().take(2_048).collect::<String>();
        let result = Database::new(PostgresDialect, self.executor.clone())
            .execute(
                sql_query::<()>("update outbox_events set last_error = ")
                    .bind(error)
                    .append(", next_attempt_at = now() + (")
                    .bind(retry_millis)
                    .append("::bigint * interval '1 millisecond'), lease_owner = null, leased_until = null where event_id = ")
                    .bind(event_id)
                    .append(" and lease_owner = ")
                    .bind(owner)
                    .append(" and published_at is null"),
            )
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        require_claimed("fail", result.rows_affected)
    }
}

fn require_claimed(action: &str, rows_affected: u64) -> Result<(), RepositoryError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "cannot {action} outbox event because its lease is no longer owned"
        )))
    }
}
