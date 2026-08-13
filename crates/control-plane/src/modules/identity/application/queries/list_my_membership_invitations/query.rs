use crate::modules::identity::domain::entities::MembershipInvitation;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::PrincipalId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListMyMembershipInvitations {
    pub principal_id: PrincipalId,
}

impl Query for ListMyMembershipInvitations {
    type Output = ApplicationResult<Vec<MembershipInvitation>>;
}
