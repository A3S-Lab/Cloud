use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use chrono::DateTime;
use chrono::Utc;
use std::time::Duration;
use uuid::Uuid;

#[async_trait]
pub trait IOutboxRepository: Send + Sync {
    async fn claim(
        &self,
        owner: Uuid,
        limit: usize,
        lease_duration: Duration,
    ) -> Result<Vec<OutboxMessage>, RepositoryError>;

    async fn mark_published(
        &self,
        event_id: Uuid,
        owner: Uuid,
        published_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError>;

    async fn mark_failed(
        &self,
        event_id: Uuid,
        owner: Uuid,
        error: &str,
        retry_after: Duration,
    ) -> Result<(), RepositoryError>;
}
