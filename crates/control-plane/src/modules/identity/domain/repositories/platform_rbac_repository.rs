use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRbacBootstrap, PlatformRoleBinding,
};
use crate::modules::identity::domain::value_objects::PlatformRole;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, InstallationId, PlatformRoleBindingId,
    PlatformRolePolicyRevisionId, PrincipalId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct BootstrapPlatformRbacWrite {
    pub bootstrap: PlatformRbacBootstrap,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct AcceptPlatformRolePolicyRevisionWrite {
    pub revision: AcceptedPlatformRolePolicyRevision,
    pub expected_current_revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct CreatePlatformRoleBindingWrite {
    pub binding: PlatformRoleBinding,
    pub expected_policy_revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ChangePlatformRoleBindingWrite {
    pub installation_id: InstallationId,
    pub binding_id: PlatformRoleBindingId,
    pub expected_version: u64,
    pub expected_policy_revision_id: PlatformRolePolicyRevisionId,
    pub role: PlatformRole,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub changed_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokePlatformRoleBindingWrite {
    pub installation_id: InstallationId,
    pub binding_id: PlatformRoleBindingId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait IPlatformRbacRepository: Send + Sync {
    async fn bootstrap_platform_rbac(
        &self,
        write: BootstrapPlatformRbacWrite,
    ) -> Result<IdempotentWrite<PlatformRbacBootstrap>, RepositoryError>;

    async fn accept_platform_role_policy_revision(
        &self,
        write: AcceptPlatformRolePolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPlatformRolePolicyRevision>, RepositoryError>;

    async fn current_platform_role_policy(
        &self,
        installation_id: InstallationId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError>;

    async fn find_platform_role_policy_revision(
        &self,
        installation_id: InstallationId,
        revision_id: PlatformRolePolicyRevisionId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError>;

    async fn create_platform_role_binding(
        &self,
        write: CreatePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError>;

    async fn change_platform_role_binding(
        &self,
        write: ChangePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError>;

    async fn revoke_platform_role_binding(
        &self,
        write: RevokePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError>;

    async fn find_platform_role_binding(
        &self,
        installation_id: InstallationId,
        binding_id: PlatformRoleBindingId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError>;

    async fn find_active_platform_role_binding_for_principal(
        &self,
        installation_id: InstallationId,
        principal_id: PrincipalId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError>;
}
