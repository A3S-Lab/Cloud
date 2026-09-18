use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::identity::domain::value_objects::DirectoryGrantSubjectRef;
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, IdempotentWrite, OrganizationId, PrincipalId, RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use uuid::Uuid;

pub const MAX_DIRECTORY_MEMBERSHIP_PROJECTION_PRINCIPALS_PER_SUBJECT: u16 = 4096;

#[derive(Debug, Clone)]
pub struct ReplaceDirectoryMembershipProjectionWrite {
    pub organization_id: OrganizationId,
    pub subject: DirectoryGrantSubjectRef,
    pub bindings: Vec<DirectoryMembershipProjectionBinding>,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

#[derive(Debug, Clone)]
pub struct ClearDirectoryMembershipProjectionWrite {
    pub organization_id: OrganizationId,
    pub subject: DirectoryGrantSubjectRef,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
    pub event: DomainEventEnvelope,
}

/// Partner-synced DirectoryProjection membership facts (subject ref → Principals).
#[async_trait]
pub trait IDirectoryMembershipProjectionRepository: Send + Sync {
    async fn replace_bindings(
        &self,
        write: ReplaceDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<Vec<DirectoryMembershipProjectionBinding>>, RepositoryError>;

    async fn list_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryGrantSubjectRef>, RepositoryError>;

    async fn list_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<PrincipalId>, RepositoryError>;

    async fn list_projection_bindings_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError>;

    async fn list_projection_bindings_for_subject(
        &self,
        organization_id: OrganizationId,
        subject: &DirectoryGrantSubjectRef,
    ) -> Result<Vec<DirectoryMembershipProjectionBinding>, RepositoryError>;

    async fn clear_bindings_for_subject(
        &self,
        write: ClearDirectoryMembershipProjectionWrite,
    ) -> Result<IdempotentWrite<()>, RepositoryError>;
}
