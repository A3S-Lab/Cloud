use crate::modules::identity::application::MembershipInvitationAcceptanceResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipInvitationId, PrincipalId};
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AcceptMembershipInvitation {
    pub invitation_id: MembershipInvitationId,
    pub expected_version: u64,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for AcceptMembershipInvitation {
    type Output = ApplicationResult<MembershipInvitationAcceptanceResult>;
}
