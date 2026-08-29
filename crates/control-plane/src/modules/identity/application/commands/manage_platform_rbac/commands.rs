use crate::modules::identity::application::{
    PlatformRoleBindingMutationResult, PlatformRolePolicyMutationResult,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId,
};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptPlatformRolePolicy {
    pub canonical_acl: String,
    pub revision_number: u64,
    pub expected_current_revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptPlatformRolePolicy {
    type Output = ApplicationResult<PlatformRolePolicyMutationResult>;
}

#[derive(Debug, Clone)]
pub struct CreatePlatformRoleBinding {
    pub principal_id: PrincipalId,
    pub role: String,
    pub expected_policy_revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreatePlatformRoleBinding {
    type Output = ApplicationResult<PlatformRoleBindingMutationResult>;
}

#[derive(Debug, Clone)]
pub struct ChangePlatformRoleBinding {
    pub binding_id: PlatformRoleBindingId,
    pub role: String,
    pub expected_version: u64,
    pub expected_policy_revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for ChangePlatformRoleBinding {
    type Output = ApplicationResult<PlatformRoleBindingMutationResult>;
}

#[derive(Debug, Clone)]
pub struct RevokePlatformRoleBinding {
    pub binding_id: PlatformRoleBindingId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokePlatformRoleBinding {
    type Output = ApplicationResult<PlatformRoleBindingMutationResult>;
}
