use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListMembershipInvitations {
    pub organization_id: OrganizationId,
}

impl Query for ListMembershipInvitations {
    type Output = ApplicationResult<Vec<MembershipInvitation>>;
}
