use crate::modules::identity::application::{
    TenantSupportGrantApprovalMutationResult, TenantSupportGrantMutationResult,
    TenantSupportGrantProposalMutationResult,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ApiTokenId, PrincipalId, TenantSupportGrantId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ProposeTenantSupportGrant {
    pub canonical_acl: String,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ProposeTenantSupportGrant {
    type Output = ApplicationResult<TenantSupportGrantProposalMutationResult>;
}

#[derive(Debug, Clone)]
pub struct ApproveTenantSupportGrant {
    pub grant_id: TenantSupportGrantId,
    pub expected_contract_digest: String,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ApproveTenantSupportGrant {
    type Output = ApplicationResult<TenantSupportGrantApprovalMutationResult>;
}

#[derive(Debug, Clone)]
pub struct RevokeTenantSupportGrant {
    pub grant_id: TenantSupportGrantId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeTenantSupportGrant {
    type Output = ApplicationResult<TenantSupportGrantMutationResult>;
}
