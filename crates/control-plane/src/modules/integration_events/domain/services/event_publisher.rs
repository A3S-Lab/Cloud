use crate::modules::integration_events::domain::entities::OutboxMessage;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
#[error("integration event publish failed: {message}")]
pub struct EventPublishError {
    message: String,
}

impl EventPublishError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait IEventPublisher: Send + Sync {
    async fn publish(&self, message: &OutboxMessage) -> Result<(), EventPublishError>;

    async fn health(&self) -> Result<bool, EventPublishError>;
}
