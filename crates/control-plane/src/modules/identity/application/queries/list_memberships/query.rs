use crate::modules::identity::domain::repositories::MembershipRecord;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListMemberships {
    pub organization_id: OrganizationId,
}

impl Query for ListMemberships {
    type Output = ApplicationResult<Vec<MembershipRecord>>;
}
