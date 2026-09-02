use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;

/// In-process application port invoked before a committed fact leaves the Outbox rail.
///
/// Implementations must be idempotent because publication acknowledgement can fail after a
/// projection succeeds. An irrelevant event is a successful no-op.
#[async_trait]
pub trait IIntegrationEventProjector: Send + Sync {
    async fn project(&self, message: &OutboxMessage) -> Result<(), RepositoryError>;
}
