use crate::modules::identity::domain::entities::Organization;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;

#[async_trait]
pub trait IOrganizationRepository: Send + Sync {
    async fn create(
        &self,
        organization: Organization,
        event: DomainEventEnvelope,
        idempotency: IdempotencyRequest,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<Organization>, RepositoryError>;

    async fn list(&self) -> Result<Vec<Organization>, RepositoryError>;
}
