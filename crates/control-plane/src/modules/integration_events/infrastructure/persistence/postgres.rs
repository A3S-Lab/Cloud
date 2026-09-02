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
        if limit == 0 {
            return Ok(Vec::new());
        }
        if owner.is_nil() {
            return Err(RepositoryError::Conflict(
                "Outbox claim requires a non-nil lease owner".into(),
            ));
        }
        let limit = i64::try_from(limit).map_err(|_| {
            RepositoryError::Conflict("Outbox claim limit exceeds the supported range".into())
        })?;
        let lease_millis = positive_interval_millis(lease_duration, "claim lease")?;
        self.executor
            .transaction(move |transaction| {
                Box::pin(async move {
                    let rows = fetch_all::<serde_json::Value, _>(
                        transaction,
                        sql_query::<serde_json::Value>(
                            "with candidates as (select event_id from outbox_events where published_at is null and next_attempt_at <= clock_timestamp() and (leased_until is null or leased_until <= clock_timestamp()) order by next_attempt_at asc, occurred_at asc, event_id asc for update skip locked limit ",
                        )
                        .bind(limit)
                        .append(") update outbox_events e set lease_owner = ")
                        .bind(owner)
                        .append(", leased_until = clock_timestamp() + (")
                        .bind(lease_millis)
                        .append("::bigint * interval '1 millisecond'), delivery_attempts = e.delivery_attempts + 1 from candidates c where e.event_id = c.event_id returning jsonb_build_object('event_id', e.event_id, 'event_key', e.event_key, 'schema_version', e.schema_version, 'scope', cloud_scope_document(e.scope_kind, e.installation_id, e.organization_id, e.project_id, e.environment_id), 'aggregate_id', e.aggregate_id, 'aggregate_version', e.aggregate_version, 'occurred_at', e.occurred_at, 'correlation_id', e.correlation_id, 'causation_id', e.causation_id, 'payload', e.payload, 'delivery_attempts', e.delivery_attempts)"),
                    )
                    .await?;
                    rows.into_iter()
                        .map(|row| {
                            let message: OutboxMessage = serde_json::from_value(row)?;
                            message
                                .validate()
                                .map_err(PostgresPersistenceError::Invariant)?;
                            Ok(message)
                        })
                        .collect::<Result<Vec<_>, PostgresPersistenceError>>()
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
        validate_claim_identity(event_id, owner, "publish")?;
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
                .append(" and published_at is null and leased_until > clock_timestamp()"),
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
        validate_claim_identity(event_id, owner, "fail")?;
        let retry_millis = positive_interval_millis(retry_after, "retry delay")?;
        let error = error.chars().take(2_048).collect::<String>();
        let result = Database::new(PostgresDialect, self.executor.clone())
            .execute(
                sql_query::<()>("update outbox_events set last_error = ")
                    .bind(error)
                    .append(", next_attempt_at = clock_timestamp() + (")
                    .bind(retry_millis)
                    .append("::bigint * interval '1 millisecond'), lease_owner = null, leased_until = null where event_id = ")
                    .bind(event_id)
                .append(" and lease_owner = ")
                .bind(owner)
                .append(" and published_at is null and leased_until > clock_timestamp()"),
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
            "cannot {action} outbox event because its active lease is no longer owned"
        )))
    }
}

fn validate_claim_identity(
    event_id: Uuid,
    owner: Uuid,
    action: &str,
) -> Result<(), RepositoryError> {
    if event_id.is_nil() || owner.is_nil() {
        Err(RepositoryError::Conflict(format!(
            "cannot {action} outbox event without a valid event and lease owner"
        )))
    } else {
        Ok(())
    }
}

fn positive_interval_millis(duration: Duration, purpose: &str) -> Result<i64, RepositoryError> {
    let milliseconds = duration.as_millis();
    if milliseconds == 0 {
        return Err(RepositoryError::Conflict(format!(
            "Outbox {purpose} must be at least one millisecond"
        )));
    }
    i64::try_from(milliseconds).map_err(|_| {
        RepositoryError::Conflict(format!("Outbox {purpose} exceeds the supported duration"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_intervals_fail_closed_instead_of_wrapping_bigint() {
        assert!(positive_interval_millis(Duration::ZERO, "claim lease").is_err());
        assert!(positive_interval_millis(Duration::from_nanos(1), "claim lease").is_err());
        assert_eq!(
            positive_interval_millis(Duration::from_millis(1), "claim lease"),
            Ok(1)
        );
        assert!(positive_interval_millis(Duration::MAX, "claim lease").is_err());
    }

    #[test]
    fn acknowledgement_identity_must_name_one_claim() {
        assert!(validate_claim_identity(Uuid::nil(), Uuid::now_v7(), "publish").is_err());
        assert!(validate_claim_identity(Uuid::now_v7(), Uuid::nil(), "publish").is_err());
        assert!(validate_claim_identity(Uuid::now_v7(), Uuid::now_v7(), "publish").is_ok());
    }
}
