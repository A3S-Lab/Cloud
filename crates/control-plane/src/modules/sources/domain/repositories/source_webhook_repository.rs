use crate::modules::shared_kernel::domain::{RepositoryError, SourceConnectionId};
use crate::modules::sources::domain::{ExternalSourceRevision, SourceWebhookDelivery};
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptSourceWebhook {
    pub delivery: SourceWebhookDelivery,
    pub authoritative_connection_id: Option<SourceConnectionId>,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct SourceWebhookAcceptance {
    pub delivery: SourceWebhookDelivery,
    pub replayed: bool,
    pub revisions: Vec<ExternalSourceRevision>,
}

#[async_trait]
pub trait ISourceWebhookRepository: Send + Sync {
    async fn accept_delivery(
        &self,
        request: AcceptSourceWebhook,
    ) -> Result<SourceWebhookAcceptance, RepositoryError>;
}
