use crate::modules::fleet::domain::value_objects::{
    NodeProtocolNegotiation, NodeProtocolNegotiationOutcome,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;

#[async_trait]
pub trait INodeProtocolSessionRepository: Send + Sync {
    async fn negotiate(
        &self,
        negotiation: NodeProtocolNegotiation,
    ) -> Result<NodeProtocolNegotiationOutcome, RepositoryError>;
}
