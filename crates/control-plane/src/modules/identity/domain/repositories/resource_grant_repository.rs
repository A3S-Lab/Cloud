use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, MembershipId, OrganizationId, PrincipalId,
    RepositoryError, ResourceGrantId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const MAX_ACTIVE_RESOURCE_GRANTS_PER_MEMBERSHIP: u16 = 256;

#[derive(Debug, Clone)]
pub struct CreateResourceGrantWrite {
    pub grant: ResourceGrant,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeResourceGrantWrite {
    pub organization_id: OrganizationId,
    pub resource_grant_id: ResourceGrantId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IResourceGrantRepository: Send + Sync {
    async fn create_resource_grant(
        &self,
        write: CreateResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError>;

    async fn find_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<ResourceGrant>, RepositoryError>;

    async fn list_resource_grants(
        &self,
        organization_id: OrganizationId,
        membership_id: Option<MembershipId>,
    ) -> Result<Vec<ResourceGrant>, RepositoryError>;

    async fn list_active_resource_grants_for_membership(
        &self,
        organization_id: OrganizationId,
        membership_id: MembershipId,
    ) -> Result<Vec<ResourceGrant>, RepositoryError>;

    async fn revoke_resource_grant(
        &self,
        write: RevokeResourceGrantWrite,
    ) -> Result<IdempotentWrite<ResourceGrant>, RepositoryError>;
}
