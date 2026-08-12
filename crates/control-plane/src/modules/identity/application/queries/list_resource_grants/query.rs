use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{MembershipId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListResourceGrants {
    pub organization_id: OrganizationId,
    pub membership_id: Option<MembershipId>,
}

impl Query for ListResourceGrants {
    type Output = ApplicationResult<Vec<ResourceGrant>>;
}
