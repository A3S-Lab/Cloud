use crate::modules::projects::domain::entities::Project;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListProjects {
    pub organization_id: OrganizationId,
}

impl Query for ListProjects {
    type Output = ApplicationResult<Vec<Project>>;
}
