use crate::modules::identity::domain::repositories::MembershipRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetMembership {
    pub organization_id: OrganizationId,
    pub membership_id: MembershipId,
}

impl Query for GetMembership {
    type Output = ApplicationResult<MembershipRecord>;
}
