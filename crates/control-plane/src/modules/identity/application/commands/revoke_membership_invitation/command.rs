use crate::modules::identity::application::MembershipInvitationMutationResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipInvitationId, OrganizationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RevokeMembershipInvitation {
    pub organization_id: OrganizationId,
    pub invitation_id: MembershipInvitationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for RevokeMembershipInvitation {
    type Output = ApplicationResult<MembershipInvitationMutationResult>;
}
