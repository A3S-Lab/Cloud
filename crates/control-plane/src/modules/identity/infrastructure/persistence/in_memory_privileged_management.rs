use super::InMemoryIdentityRepository;
use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, AcceptedTrustDomainRevision,
    AcceptedWorkloadIdentityPolicyRevision, PlatformRbacBootstrap, PlatformRoleBinding,
    TenantSupportGrant, TenantSupportGrantApproval, TenantSupportGrantApprovalOutcome,
    TenantSupportGrantProposal,
};
use crate::modules::identity::domain::repositories::{
    AcceptPlatformRolePolicyRevisionWrite, AcceptTrustDomainRevisionWrite,
    AcceptWorkloadIdentityPolicyRevisionWrite, ApproveTenantSupportGrantWrite,
    BootstrapPlatformRbacWrite, ChangePlatformRoleBindingWrite, CreatePlatformRoleBindingWrite,
    IPlatformRbacRepository, ITenantSupportGrantRepository, ITrustDomainRepository,
    IWorkloadIdentityPolicyRepository, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions, ProposeTenantSupportGrantWrite,
    ReadCurrentPlatformRolePolicy, ReadCurrentTrustDomain, ReadCurrentWorkloadIdentityPolicy,
    ReadCurrentWorkloadIdentityPolicyForWorkload, ReadPlatformRoleBinding,
    ReadPlatformRolePolicyRevision, ReadPrincipalPlatformRoleBinding, ReadTenantSupportGrant,
    ReadTrustDomainRevision, ReadWorkloadIdentityPolicyRevision, RevokePlatformRoleBindingWrite,
    RevokeTenantSupportGrantWrite, TenantSupportGrantRecord,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, InstallationId, PlatformRoleBindingId, PlatformRolePolicyRevisionId,
    PrincipalId, RepositoryError, TenantSupportGrantId,
};
use async_trait::async_trait;

fn unavailable<T>() -> Result<T, RepositoryError> {
    Err(RepositoryError::Forbidden(
        "privileged management requires the PostgreSQL Identity authority".into(),
    ))
}

#[async_trait]
impl ITrustDomainRepository for InMemoryIdentityRepository {
    async fn accept(
        &self,
        _write: AcceptTrustDomainRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedTrustDomainRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_revision(
        &self,
        _read: ReadTrustDomainRevision,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_current(
        &self,
        _read: ReadCurrentTrustDomain,
    ) -> Result<Option<AcceptedTrustDomainRevision>, RepositoryError> {
        unavailable()
    }

    async fn list_revisions(
        &self,
        _read: ListTrustDomainRevisions,
    ) -> Result<Vec<AcceptedTrustDomainRevision>, RepositoryError> {
        unavailable()
    }
}

#[async_trait]
impl IWorkloadIdentityPolicyRepository for InMemoryIdentityRepository {
    async fn accept(
        &self,
        _write: AcceptWorkloadIdentityPolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_revision(
        &self,
        _read: ReadWorkloadIdentityPolicyRevision,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_current(
        &self,
        _read: ReadCurrentWorkloadIdentityPolicy,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_current_for_workload(
        &self,
        _read: ReadCurrentWorkloadIdentityPolicyForWorkload,
    ) -> Result<Option<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn list_revisions(
        &self,
        _read: ListWorkloadIdentityPolicyRevisions,
    ) -> Result<Vec<AcceptedWorkloadIdentityPolicyRevision>, RepositoryError> {
        unavailable()
    }
}

/// The in-memory Identity adapter deliberately implements no privileged
/// management authority. It only lets generic process-composition tests wire
/// the same closed ports and fail closed when a privileged use case is called.
#[async_trait]
impl IPlatformRbacRepository for InMemoryIdentityRepository {
    async fn bootstrap_platform_rbac(
        &self,
        _write: BootstrapPlatformRbacWrite,
    ) -> Result<IdempotentWrite<PlatformRbacBootstrap>, RepositoryError> {
        unavailable()
    }

    async fn accept_platform_role_policy_revision(
        &self,
        _write: AcceptPlatformRolePolicyRevisionWrite,
    ) -> Result<IdempotentWrite<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn current_platform_role_policy(
        &self,
        _installation_id: InstallationId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn find_platform_role_policy_revision(
        &self,
        _installation_id: InstallationId,
        _revision_id: PlatformRolePolicyRevisionId,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn create_platform_role_binding(
        &self,
        _write: CreatePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn change_platform_role_binding(
        &self,
        _write: ChangePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn revoke_platform_role_binding(
        &self,
        _write: RevokePlatformRoleBindingWrite,
    ) -> Result<IdempotentWrite<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn find_platform_role_binding(
        &self,
        _installation_id: InstallationId,
        _binding_id: PlatformRoleBindingId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn find_active_platform_role_binding_for_principal(
        &self,
        _installation_id: InstallationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn read_current_platform_role_policy(
        &self,
        _read: ReadCurrentPlatformRolePolicy,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_platform_role_policy_revision(
        &self,
        _read: ReadPlatformRolePolicyRevision,
    ) -> Result<Option<AcceptedPlatformRolePolicyRevision>, RepositoryError> {
        unavailable()
    }

    async fn read_platform_role_binding(
        &self,
        _read: ReadPlatformRoleBinding,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }

    async fn read_principal_platform_role_binding(
        &self,
        _read: ReadPrincipalPlatformRoleBinding,
    ) -> Result<Option<PlatformRoleBinding>, RepositoryError> {
        unavailable()
    }
}

#[async_trait]
impl ITenantSupportGrantRepository for InMemoryIdentityRepository {
    async fn propose_tenant_support_grant(
        &self,
        _write: ProposeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantProposal>, RepositoryError> {
        unavailable()
    }

    async fn approve_tenant_support_grant(
        &self,
        _write: ApproveTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantApprovalOutcome>, RepositoryError> {
        unavailable()
    }

    async fn revoke_tenant_support_grant(
        &self,
        _write: RevokeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrant>, RepositoryError> {
        unavailable()
    }

    async fn find_tenant_support_grant_proposal(
        &self,
        _installation_id: InstallationId,
        _grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrantProposal>, RepositoryError> {
        unavailable()
    }

    async fn list_tenant_support_grant_approvals(
        &self,
        _installation_id: InstallationId,
        _grant_id: TenantSupportGrantId,
    ) -> Result<Vec<TenantSupportGrantApproval>, RepositoryError> {
        unavailable()
    }

    async fn find_tenant_support_grant(
        &self,
        _installation_id: InstallationId,
        _grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrant>, RepositoryError> {
        unavailable()
    }

    async fn read_tenant_support_grant(
        &self,
        _read: ReadTenantSupportGrant,
    ) -> Result<Option<TenantSupportGrantRecord>, RepositoryError> {
        unavailable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_privileged_management_has_no_authority() {
        let repository = InMemoryIdentityRepository::new();
        assert!(matches!(
            repository
                .current_platform_role_policy(InstallationId::new())
                .await,
            Err(RepositoryError::Forbidden(_))
        ));
        assert!(matches!(
            repository
                .find_tenant_support_grant(InstallationId::new(), TenantSupportGrantId::new())
                .await,
            Err(RepositoryError::Forbidden(_))
        ));
    }
}
