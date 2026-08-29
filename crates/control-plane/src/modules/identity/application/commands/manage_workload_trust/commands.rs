use crate::modules::identity::application::{
    TrustDomainRevisionMutationResult, WorkloadIdentityPolicyRevisionMutationResult,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, OrganizationId, PrincipalId, TrustDomainId, TrustDomainRevisionId,
    WorkloadIdentityPolicyId, WorkloadIdentityPolicyRevisionId,
};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptTrustDomainRevision {
    pub trust_domain_id: TrustDomainId,
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_previous_revision_id: Option<TrustDomainRevisionId>,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptTrustDomainRevision {
    type Output = ApplicationResult<TrustDomainRevisionMutationResult>;
}

#[derive(Debug, Clone)]
pub struct AcceptWorkloadIdentityPolicyRevision {
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_previous_revision_id: Option<WorkloadIdentityPolicyRevisionId>,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptWorkloadIdentityPolicyRevision {
    type Output = ApplicationResult<WorkloadIdentityPolicyRevisionMutationResult>;
}
