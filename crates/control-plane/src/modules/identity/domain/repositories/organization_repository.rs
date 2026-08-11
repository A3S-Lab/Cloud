use crate::modules::identity::domain::entities::{Membership, Organization};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateOrganizationWrite {
    pub organization: Organization,
    pub owner_membership: Membership,
    pub events: [DomainEventEnvelope; 2],
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IOrganizationRepository: Send + Sync {
    async fn create(
        &self,
        write: CreateOrganizationWrite,
    ) -> Result<IdempotentWrite<Organization>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<Organization>, RepositoryError>;

    async fn list(&self) -> Result<Vec<Organization>, RepositoryError>;
}
