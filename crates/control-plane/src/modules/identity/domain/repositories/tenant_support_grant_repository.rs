use crate::modules::identity::domain::entities::{
    TenantSupportGrant, TenantSupportGrantApproval, TenantSupportGrantApprovalOutcome,
    TenantSupportGrantProposal,
};
use crate::modules::identity::domain::value_objects::TenantSupportGrantContract;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, IdempotencyRequest, IdempotentWrite, InstallationId, PrincipalId, RepositoryError,
    Sha256Digest, TenantSupportGrantId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantSupportGrantRecord {
    pub proposal: TenantSupportGrantProposal,
    pub approvals: Vec<TenantSupportGrantApproval>,
    pub grant: Option<TenantSupportGrant>,
}

#[derive(Debug, Clone)]
pub struct ProposeTenantSupportGrantWrite {
    pub contract: TenantSupportGrantContract,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub requested_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ApproveTenantSupportGrantWrite {
    pub installation_id: InstallationId,
    pub grant_id: TenantSupportGrantId,
    pub expected_contract_digest: Sha256Digest,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
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
    pub credential_id: ApiTokenId,
    pub revoked_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub idempotency: IdempotencyRequest,
}

#[derive(Debug, Clone)]
pub struct ReadTenantSupportGrant {
    pub installation_id: InstallationId,
    pub grant_id: TenantSupportGrantId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
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

    /// Authorizes the closed `TenantSupportRead` capability and returns the
    /// proposal, approval evidence, and accepted lifecycle under one storage
    /// transaction and Installation lock interval.
    async fn read_tenant_support_grant(
        &self,
        read: ReadTenantSupportGrant,
    ) -> Result<Option<TenantSupportGrantRecord>, RepositoryError>;
}
