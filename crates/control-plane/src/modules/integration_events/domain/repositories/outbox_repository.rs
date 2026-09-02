use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use std::time::Duration;
use uuid::Uuid;

#[async_trait]
pub trait IOutboxRepository: Send + Sync {
    /// Claims at most `limit` ready facts for one owner and increments each delivery attempt.
    ///
    /// A returned fact is exclusively owned only until its lease expires. Callers must settle it
    /// while that same lease is current; an expired owner is never allowed to publish or fail it.
    async fn claim(
        &self,
        owner: Uuid,
        limit: usize,
        lease_duration: Duration,
    ) -> Result<Vec<OutboxMessage>, RepositoryError>;

    /// Settles one fact only when `owner` still holds its active lease.
    async fn mark_published(
        &self,
        event_id: Uuid,
        owner: Uuid,
        published_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError>;

    /// Schedules one retry only when `owner` still holds its active lease.
    async fn mark_failed(
        &self,
        event_id: Uuid,
        owner: Uuid,
        error: &str,
        retry_after: Duration,
    ) -> Result<(), RepositoryError>;
}
