use crate::modules::identity::domain::entities::{
    AcceptedPlatformRolePolicyRevision, PlatformRoleBinding,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, PlatformRoleBindingId, PlatformRolePolicyRevisionId, PrincipalId,
};
use a3s_boot::Query;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetCurrentPlatformRolePolicy {
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetCurrentPlatformRolePolicy {
    type Output = ApplicationResult<AcceptedPlatformRolePolicyRevision>;
}

#[derive(Debug, Clone)]
pub struct GetPlatformRolePolicyRevision {
    pub revision_id: PlatformRolePolicyRevisionId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetPlatformRolePolicyRevision {
    type Output = ApplicationResult<AcceptedPlatformRolePolicyRevision>;
}

#[derive(Debug, Clone)]
pub struct GetPlatformRoleBinding {
    pub binding_id: PlatformRoleBindingId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetPlatformRoleBinding {
    type Output = ApplicationResult<PlatformRoleBinding>;
}

#[derive(Debug, Clone)]
pub struct GetPrincipalPlatformRoleBinding {
    pub principal_id: PrincipalId,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub request_id: Uuid,
}

impl Query for GetPrincipalPlatformRoleBinding {
    type Output = ApplicationResult<PlatformRoleBinding>;
}
