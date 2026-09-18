use crate::modules::identity::domain::entities::DirectoryResourceGrant;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ResourceGrantId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetDirectoryResourceGrant {
    pub organization_id: OrganizationId,
    pub resource_grant_id: ResourceGrantId,
}

impl Query for GetDirectoryResourceGrant {
    type Output = ApplicationResult<DirectoryResourceGrant>;
}
