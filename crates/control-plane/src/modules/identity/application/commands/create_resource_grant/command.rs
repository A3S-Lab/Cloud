use crate::modules::identity::application::ResourceGrantMutationResult;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateResourceGrant {
    pub organization_id: OrganizationId,
    pub membership_id: MembershipId,
    pub scope: ResourceGrantScope,
    pub actor_principal_id: PrincipalId,
    pub actor_is_platform_admin: bool,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateResourceGrant {
    type Output = ApplicationResult<ResourceGrantMutationResult>;
}
