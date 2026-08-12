use crate::modules::identity::application::ResourceGrantMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ResourceGrantId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeResourceGrant {
    pub organization_id: OrganizationId,
    pub resource_grant_id: ResourceGrantId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeResourceGrant {
    type Output = ApplicationResult<ResourceGrantMutationResult>;
}
