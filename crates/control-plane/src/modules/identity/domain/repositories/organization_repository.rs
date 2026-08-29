use crate::modules::identity::domain::entities::{Membership, Organization};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, InstallationId, OrganizationId, PrincipalId,
    RepositoryError,
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

#[derive(Debug, Clone)]
pub struct ReadOrganizationCatalog {
    pub installation_id: InstallationId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
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

    /// Returns the exact credential's tenant or, when the canonical
    /// Installation policy atomically grants `TenantLifecycleRead`, the whole
    /// Installation catalog.
    async fn list_visible(
        &self,
        read: ReadOrganizationCatalog,
    ) -> Result<Vec<Organization>, RepositoryError>;
}
