use crate::modules::identity::domain::value_objects::{
    PlatformPermission, TenantSupportPermission,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    ApiTokenId, AuthorizationDecisionRef, PrincipalId, ScopeContext, TenantSupportGrantId,
};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuthorizePrivilegedAccess {
    pub principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub platform_permission: PlatformPermission,
    pub support_permission: Option<TenantSupportPermission>,
    pub support_grant_id: Option<TenantSupportGrantId>,
    pub action: String,
    pub scope: ScopeContext,
    pub resource_id: Uuid,
    pub request_id: Uuid,
}

impl Command for AuthorizePrivilegedAccess {
    type Output = ApplicationResult<AuthorizationDecisionRef>;
}
