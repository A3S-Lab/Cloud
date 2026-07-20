use crate::modules::shared_kernel::domain::{IdempotentWrite, RepositoryError};
use crate::modules::sources::domain::SourceWebhookDelivery;
use async_trait::async_trait;

#[async_trait]
pub trait ISourceWebhookRepository: Send + Sync {
    async fn accept_delivery(
        &self,
        delivery: SourceWebhookDelivery,
    ) -> Result<IdempotentWrite<SourceWebhookDelivery>, RepositoryError>;
}
