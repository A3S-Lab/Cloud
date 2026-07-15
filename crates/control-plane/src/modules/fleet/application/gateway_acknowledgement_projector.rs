use crate::modules::shared_kernel::domain::RepositoryError;
use a3s_cloud_contracts::NodeGatewayAck;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait IGatewayAcknowledgementProjector: Send + Sync {
    async fn project(
        &self,
        acknowledgement: &NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<(), RepositoryError>;
}
