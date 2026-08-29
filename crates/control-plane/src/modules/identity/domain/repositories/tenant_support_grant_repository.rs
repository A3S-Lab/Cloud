use crate::modules::identity::domain::entities::{
    TenantSupportGrant, TenantSupportGrantApproval, TenantSupportGrantApprovalOutcome,
    TenantSupportGrantProposal,
};
use crate::modules::shared_kernel::domain::{
    DecisionEvidenceRef, IdempotencyRequest, IdempotentWrite, InstallationId, PrincipalId,
    RepositoryError, Sha256Digest, TenantSupportGrantId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProposeTenantSupportGrantWrite {
    pub proposal: TenantSupportGrantProposal,
    pub actor_principal_id: PrincipalId,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ApproveTenantSupportGrantWrite {
    pub installation_id: InstallationId,
    pub grant_id: TenantSupportGrantId,
    pub expected_contract_digest: Sha256Digest,
    pub actor_principal_id: PrincipalId,
    pub authentication: DecisionEvidenceRef,
    pub approved_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct RevokeTenantSupportGrantWrite {
    pub installation_id: InstallationId,
    pub grant_id: TenantSupportGrantId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub authentication: DecisionEvidenceRef,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[async_trait]
pub trait ITenantSupportGrantRepository: Send + Sync {
    async fn propose_tenant_support_grant(
        &self,
        write: ProposeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantProposal>, RepositoryError>;

    async fn approve_tenant_support_grant(
        &self,
        write: ApproveTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrantApprovalOutcome>, RepositoryError>;

    async fn revoke_tenant_support_grant(
        &self,
        write: RevokeTenantSupportGrantWrite,
    ) -> Result<IdempotentWrite<TenantSupportGrant>, RepositoryError>;

    async fn find_tenant_support_grant_proposal(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrantProposal>, RepositoryError>;

    async fn list_tenant_support_grant_approvals(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Vec<TenantSupportGrantApproval>, RepositoryError>;

    async fn find_tenant_support_grant(
        &self,
        installation_id: InstallationId,
        grant_id: TenantSupportGrantId,
    ) -> Result<Option<TenantSupportGrant>, RepositoryError>;
}
