use super::{arguments, tool_result};
use crate::modules::identity::application::commands::manage_platform_rbac::{
    AcceptPlatformRolePolicy, ChangePlatformRoleBinding, CreatePlatformRoleBinding,
    RevokePlatformRoleBinding,
};
use crate::modules::identity::application::commands::manage_tenant_support::{
    ApproveTenantSupportGrant, ProposeTenantSupportGrant, RevokeTenantSupportGrant,
};
use crate::modules::identity::application::commands::manage_workload_trust::{
    AcceptTrustDomainRevision, AcceptWorkloadIdentityPolicyRevision,
};
use crate::modules::identity::application::queries::read_platform_rbac::{
    GetCurrentPlatformRolePolicy, GetPlatformRoleBinding, GetPlatformRolePolicyRevision,
    GetPrincipalPlatformRoleBinding,
};
use crate::modules::identity::application::queries::read_tenant_support::GetTenantSupportGrant;
use crate::modules::identity::application::queries::read_workload_trust::{
    GetCurrentTrustDomain, GetCurrentWorkloadIdentityPolicy,
    GetCurrentWorkloadIdentityPolicyForWorkload, GetTrustDomainRevision,
    GetWorkloadIdentityPolicyRevision, InspectCurrentTrustDomainProvider, ListTrustDomainRevisions,
    ListWorkloadIdentityPolicyRevisions,
};
use crate::modules::identity::domain::repositories::{
    DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE, MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
};
use crate::modules::identity::presentation::{
    PlatformRoleBindingMutationResponse, PlatformRoleBindingResponse,
    PlatformRolePolicyMutationResponse, PlatformRolePolicyResponse,
    TenantSupportGrantApprovalMutationResponse, TenantSupportGrantMutationResponse,
    TenantSupportGrantProposalMutationResponse, TenantSupportGrantResponse,
    TrustDomainRevisionMutationResponse, TrustDomainRevisionResponse,
    WorkloadIdentityPolicyRevisionMutationResponse, WorkloadIdentityPolicyRevisionResponse,
    WorkloadIdentityProviderInspectionResponse,
};
use crate::modules::shared_kernel::domain::{
    ApiTokenId, OrganizationId, PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId,
    TenantSupportGrantId, TrustDomainId, TrustDomainRevisionId, WorkloadId,
    WorkloadIdentityPolicyId, WorkloadIdentityPolicyRevisionId,
};
use a3s_boot::{CommandBus, QueryBus, Result};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformRolePolicyRevisionArguments {
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptPlatformRolePolicyArguments {
    canonical_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    revision_number: u64,
    expected_current_revision_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformRoleBindingArguments {
    binding_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipalPlatformRoleBindingArguments {
    principal_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePlatformRoleBindingArguments {
    principal_id: Uuid,
    role: String,
    expected_policy_revision_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePlatformRoleBindingArguments {
    binding_id: Uuid,
    role: String,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    expected_policy_revision_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokePlatformRoleBindingArguments {
    binding_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantSupportGrantArguments {
    grant_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposeTenantSupportGrantArguments {
    canonical_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApproveTenantSupportGrantArguments {
    grant_id: Uuid,
    expected_contract_digest: String,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeTenantSupportGrantArguments {
    grant_id: Uuid,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    expected_version: u64,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustDomainArguments {
    trust_domain_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustDomainRevisionArguments {
    trust_domain_id: Uuid,
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustDomainRevisionListArguments {
    trust_domain_id: Uuid,
    #[serde(
        default = "default_workload_trust_revision_limit",
        deserialize_with = "deserialize_workload_trust_revision_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptTrustDomainRevisionArguments {
    trust_domain_id: Uuid,
    canonical_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    revision_number: u64,
    expected_previous_revision_id: Option<Uuid>,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadIdentityPolicyArguments {
    organization_id: Uuid,
    policy_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadIdentityPolicyForWorkloadArguments {
    organization_id: Uuid,
    workload_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadIdentityPolicyRevisionArguments {
    organization_id: Uuid,
    policy_id: Uuid,
    revision_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadIdentityPolicyRevisionListArguments {
    organization_id: Uuid,
    policy_id: Uuid,
    #[serde(
        default = "default_workload_trust_revision_limit",
        deserialize_with = "deserialize_workload_trust_revision_limit"
    )]
    limit: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptWorkloadIdentityPolicyRevisionArguments {
    organization_id: Uuid,
    policy_id: Uuid,
    canonical_acl: String,
    #[serde(deserialize_with = "arguments::deserialize_expected_version")]
    revision_number: u64,
    expected_previous_revision_id: Option<Uuid>,
    #[serde(deserialize_with = "arguments::deserialize_idempotency_key")]
    idempotency_key: String,
}

const fn default_workload_trust_revision_limit() -> usize {
    DEFAULT_WORKLOAD_IDENTITY_REVISIONS_PAGE
}

fn deserialize_workload_trust_revision_limit<'de, D>(
    deserializer: D,
) -> std::result::Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    arguments::deserialize_bounded_list_limit(
        deserializer,
        MAX_WORKLOAD_IDENTITY_REVISIONS_PAGE,
        "workload trust revision limit",
    )
}

pub async fn get_current_platform_role_policy(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetCurrentPlatformRolePolicy {
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(policy) => {
            tool_result::success(200, PlatformRolePolicyResponse::from(policy), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_platform_role_policy_revision(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: PlatformRolePolicyRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPlatformRolePolicyRevision {
            revision_id: PlatformRolePolicyRevisionId::from_uuid(arguments.revision_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(policy) => {
            tool_result::success(200, PlatformRolePolicyResponse::from(policy), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn accept_platform_role_policy(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: AcceptPlatformRolePolicyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AcceptPlatformRolePolicy {
            canonical_acl: arguments.canonical_acl,
            revision_number: arguments.revision_number,
            expected_current_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                arguments.expected_current_revision_id,
            ),
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            PlatformRolePolicyMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_platform_role_binding(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: PlatformRoleBindingArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPlatformRoleBinding {
            binding_id: PlatformRoleBindingId::from_uuid(arguments.binding_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(binding) => {
            tool_result::success(200, PlatformRoleBindingResponse::from(binding), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_principal_platform_role_binding(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: PrincipalPlatformRoleBindingArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetPrincipalPlatformRoleBinding {
            principal_id: PrincipalId::from_uuid(arguments.principal_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(binding) => {
            tool_result::success(200, PlatformRoleBindingResponse::from(binding), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn create_platform_role_binding(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: CreatePlatformRoleBindingArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(CreatePlatformRoleBinding {
            principal_id: PrincipalId::from_uuid(arguments.principal_id),
            role: arguments.role,
            expected_policy_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                arguments.expected_policy_revision_id,
            ),
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            PlatformRoleBindingMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn change_platform_role_binding(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: ChangePlatformRoleBindingArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ChangePlatformRoleBinding {
            binding_id: PlatformRoleBindingId::from_uuid(arguments.binding_id),
            role: arguments.role,
            expected_version: arguments.expected_version,
            expected_policy_revision_id: PlatformRolePolicyRevisionId::from_uuid(
                arguments.expected_policy_revision_id,
            ),
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            PlatformRoleBindingMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_platform_role_binding(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: RevokePlatformRoleBindingArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokePlatformRoleBinding {
            binding_id: PlatformRoleBindingId::from_uuid(arguments.binding_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            PlatformRoleBindingMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_tenant_support_grant(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: TenantSupportGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetTenantSupportGrant {
            grant_id: TenantSupportGrantId::from_uuid(arguments.grant_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(record) => {
            tool_result::success(200, TenantSupportGrantResponse::from(record), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn propose_tenant_support_grant(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: ProposeTenantSupportGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ProposeTenantSupportGrant {
            canonical_acl: arguments.canonical_acl,
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 202 },
            TenantSupportGrantProposalMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn approve_tenant_support_grant(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: ApproveTenantSupportGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ApproveTenantSupportGrant {
            grant_id: TenantSupportGrantId::from_uuid(arguments.grant_id),
            expected_contract_digest: arguments.expected_contract_digest,
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            TenantSupportGrantApprovalMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn revoke_tenant_support_grant(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: RevokeTenantSupportGrantArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(RevokeTenantSupportGrant {
            grant_id: TenantSupportGrantId::from_uuid(arguments.grant_id),
            expected_version: arguments.expected_version,
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            TenantSupportGrantMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_current_trust_domain(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: TrustDomainArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetCurrentTrustDomain {
            trust_domain_id: TrustDomainId::from_uuid(arguments.trust_domain_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revision) => {
            tool_result::success(200, TrustDomainRevisionResponse::from(revision), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn inspect_current_trust_domain_provider(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: TrustDomainArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(InspectCurrentTrustDomainProvider {
            trust_domain_id: TrustDomainId::from_uuid(arguments.trust_domain_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            200,
            WorkloadIdentityProviderInspectionResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_trust_domain_revision(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: TrustDomainRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetTrustDomainRevision {
            trust_domain_id: TrustDomainId::from_uuid(arguments.trust_domain_id),
            revision_id: TrustDomainRevisionId::from_uuid(arguments.revision_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revision) => {
            tool_result::success(200, TrustDomainRevisionResponse::from(revision), request_id)
        }
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_trust_domain_revisions(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: TrustDomainRevisionListArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListTrustDomainRevisions {
            trust_domain_id: TrustDomainId::from_uuid(arguments.trust_domain_id),
            limit: arguments.limit,
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revisions) => tool_result::success(
            200,
            revisions
                .into_iter()
                .map(TrustDomainRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn accept_trust_domain_revision(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: AcceptTrustDomainRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AcceptTrustDomainRevision {
            trust_domain_id: TrustDomainId::from_uuid(arguments.trust_domain_id),
            canonical_acl: arguments.canonical_acl,
            revision_number: arguments.revision_number,
            expected_previous_revision_id: arguments
                .expected_previous_revision_id
                .map(TrustDomainRevisionId::from_uuid),
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            TrustDomainRevisionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_current_workload_identity_policy(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: WorkloadIdentityPolicyArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetCurrentWorkloadIdentityPolicy {
            organization_id: OrganizationId::from_uuid(arguments.organization_id),
            policy_id: WorkloadIdentityPolicyId::from_uuid(arguments.policy_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            WorkloadIdentityPolicyRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_current_workload_identity_policy_for_workload(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: WorkloadIdentityPolicyForWorkloadArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetCurrentWorkloadIdentityPolicyForWorkload {
            organization_id: OrganizationId::from_uuid(arguments.organization_id),
            workload_id: WorkloadId::from_uuid(arguments.workload_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            WorkloadIdentityPolicyRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_workload_identity_policy_revision(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: WorkloadIdentityPolicyRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetWorkloadIdentityPolicyRevision {
            organization_id: OrganizationId::from_uuid(arguments.organization_id),
            policy_id: WorkloadIdentityPolicyId::from_uuid(arguments.policy_id),
            revision_id: WorkloadIdentityPolicyRevisionId::from_uuid(arguments.revision_id),
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revision) => tool_result::success(
            200,
            WorkloadIdentityPolicyRevisionResponse::from(revision),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn list_workload_identity_policy_revisions(
    bus: Arc<QueryBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: WorkloadIdentityPolicyRevisionListArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListWorkloadIdentityPolicyRevisions {
            organization_id: OrganizationId::from_uuid(arguments.organization_id),
            policy_id: WorkloadIdentityPolicyId::from_uuid(arguments.policy_id),
            limit: arguments.limit,
            actor_principal_id,
            credential_id,
            request_id,
        })
        .await?
    {
        Ok(revisions) => tool_result::success(
            200,
            revisions
                .into_iter()
                .map(WorkloadIdentityPolicyRevisionResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn accept_workload_identity_policy_revision(
    bus: Arc<CommandBus>,
    actor_principal_id: PrincipalId,
    credential_id: ApiTokenId,
    arguments: AcceptWorkloadIdentityPolicyRevisionArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(AcceptWorkloadIdentityPolicyRevision {
            organization_id: OrganizationId::from_uuid(arguments.organization_id),
            policy_id: WorkloadIdentityPolicyId::from_uuid(arguments.policy_id),
            canonical_acl: arguments.canonical_acl,
            revision_number: arguments.revision_number,
            expected_previous_revision_id: arguments
                .expected_previous_revision_id
                .map(WorkloadIdentityPolicyRevisionId::from_uuid),
            actor_principal_id,
            credential_id,
            idempotency_key: arguments.idempotency_key,
            request_id,
        })
        .await?
    {
        Ok(result) => tool_result::success(
            if result.replayed { 200 } else { 201 },
            WorkloadIdentityPolicyRevisionMutationResponse::from(result),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
