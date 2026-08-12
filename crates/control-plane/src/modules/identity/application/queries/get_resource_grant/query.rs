use crate::modules::identity::domain::entities::ResourceGrant;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ResourceGrantId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetResourceGrant {
    pub organization_id: OrganizationId,
    pub resource_grant_id: ResourceGrantId,
}

impl Query for GetResourceGrant {
    type Output = ApplicationResult<ResourceGrant>;
}
