use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipInvitationId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetMembershipInvitation {
    pub organization_id: OrganizationId,
    pub invitation_id: MembershipInvitationId,
}

impl Query for GetMembershipInvitation {
    type Output = ApplicationResult<MembershipInvitation>;
}
