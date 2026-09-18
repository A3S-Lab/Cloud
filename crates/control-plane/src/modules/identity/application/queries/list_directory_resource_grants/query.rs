use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListDirectoryResourceGrants {
    pub organization_id: OrganizationId,
}

impl Query for ListDirectoryResourceGrants {
    type Output = ApplicationResult<Vec<DirectoryResourceGrant>>;
}
