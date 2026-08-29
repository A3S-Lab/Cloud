use crate::modules::identity::domain::entities::{
    AcceptedTrustDomainRevision, AcceptedWorkloadIdentityPolicyRevision,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, OrganizationId, PrincipalId, TrustDomainId, TrustDomainRevisionId, WorkloadId,
    WorkloadIdentityPolicyId, WorkloadIdentityPolicyRevisionId,
};
use a3s_boot::Query;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetCurrentTrustDomain {
    pub trust_domain_id: TrustDomainId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetCurrentTrustDomain {
    type Output = ApplicationResult<AcceptedTrustDomainRevision>;
}

#[derive(Debug, Clone)]
pub struct GetTrustDomainRevision {
    pub trust_domain_id: TrustDomainId,
    pub revision_id: TrustDomainRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetTrustDomainRevision {
    type Output = ApplicationResult<AcceptedTrustDomainRevision>;
}

#[derive(Debug, Clone)]
pub struct ListTrustDomainRevisions {
    pub trust_domain_id: TrustDomainId,
    pub limit: usize,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for ListTrustDomainRevisions {
    type Output = ApplicationResult<Vec<AcceptedTrustDomainRevision>>;
}

#[derive(Debug, Clone)]
pub struct GetCurrentWorkloadIdentityPolicy {
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetCurrentWorkloadIdentityPolicy {
    type Output = ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>;
}

#[derive(Debug, Clone)]
pub struct GetCurrentWorkloadIdentityPolicyForWorkload {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetCurrentWorkloadIdentityPolicyForWorkload {
    type Output = ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>;
}

#[derive(Debug, Clone)]
pub struct GetWorkloadIdentityPolicyRevision {
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub revision_id: WorkloadIdentityPolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetWorkloadIdentityPolicyRevision {
    type Output = ApplicationResult<AcceptedWorkloadIdentityPolicyRevision>;
}

#[derive(Debug, Clone)]
pub struct ListWorkloadIdentityPolicyRevisions {
    pub organization_id: OrganizationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub limit: usize,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for ListWorkloadIdentityPolicyRevisions {
    type Output = ApplicationResult<Vec<AcceptedWorkloadIdentityPolicyRevision>>;
}
