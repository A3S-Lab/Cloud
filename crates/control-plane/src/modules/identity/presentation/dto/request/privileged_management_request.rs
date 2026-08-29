use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptPlatformRolePolicyRequest {
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_current_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePlatformRoleBindingRequest {
    pub principal_id: Uuid,
    pub role: String,
    pub expected_policy_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePlatformRoleBindingRequest {
    pub role: String,
    pub expected_version: u64,
    pub expected_policy_revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExpectedVersionRequest {
    pub expected_version: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeTenantSupportGrantRequest {
    pub canonical_acl: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApproveTenantSupportGrantRequest {
    pub expected_contract_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptTrustDomainRevisionRequest {
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_previous_revision_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptWorkloadIdentityPolicyRevisionRequest {
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_previous_revision_id: Option<Uuid>,
}
