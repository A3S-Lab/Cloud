use crate::modules::identity::domain::entities::Organization;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListOrganizations {
    pub organization_id: Option<OrganizationId>,
}

impl Query for ListOrganizations {
    type Output = ApplicationResult<Vec<Organization>>;
}
