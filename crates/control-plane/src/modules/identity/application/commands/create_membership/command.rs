use crate::modules::identity::application::MembershipMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateMembership {
    pub organization_id: OrganizationId,
    pub principal_kind: String,
    pub name: String,
    pub role: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateMembership {
    type Output = ApplicationResult<MembershipMutationResult>;
}
