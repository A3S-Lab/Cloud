use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
    ResourceGrantId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub const MAX_ACTIVE_DIRECTORY_RESOURCE_GRANTS_PER_ORG: u16 = 1024;

#[derive(Debug, Clone)]
pub struct CreateDirectoryResourceGrantWrite {
    pub grant: DirectoryResourceGrant,
    pub event: DomainEventEnvelope,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeDirectoryResourceGrantWrite {
    pub organization_id: OrganizationId,
    pub resource_grant_id: ResourceGrantId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IDirectoryResourceGrantRepository: Send + Sync {
    async fn create_directory_resource_grant(
        &self,
        write: CreateDirectoryResourceGrantWrite,
    ) -> Result<IdempotentWrite<DirectoryResourceGrant>, RepositoryError>;

    async fn find_directory_resource_grant(
        &self,
        organization_id: OrganizationId,
        resource_grant_id: ResourceGrantId,
    ) -> Result<Option<DirectoryResourceGrant>, RepositoryError>;

    async fn list_directory_resource_grants_by_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<DirectoryResourceGrant>, RepositoryError>;

    async fn list_active_directory_resource_grants_for_subjects(
        &self,
        organization_id: OrganizationId,
        subjects: &[DirectoryGrantSubjectRef],
    ) -> Result<Vec<DirectoryResourceGrant>, RepositoryError>;

    async fn revoke_directory_resource_grant(
        &self,
        write: RevokeDirectoryResourceGrantWrite,
    ) -> Result<IdempotentWrite<DirectoryResourceGrant>, RepositoryError>;
}
