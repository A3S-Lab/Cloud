use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, NodePoolId, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;

pub struct NodePoolWrite {
    pub pool: NodePool,
    pub expected_version: Option<u64>,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait INodePoolRepository: Send + Sync {
    async fn replay(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<NodePool>, RepositoryError>;

    async fn save(
        &self,
        write: NodePoolWrite,
    ) -> Result<IdempotentWrite<NodePool>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        pool_id: NodePoolId,
    ) -> Result<NodePool, RepositoryError>;

    async fn list(&self, organization_id: OrganizationId)
        -> Result<Vec<NodePool>, RepositoryError>;
}
