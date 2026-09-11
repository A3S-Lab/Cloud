use crate::modules::projects::domain::entities::Project;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;
use crate::modules::projects::application::ProjectAccess;

#[derive(Debug, Clone)]
pub struct ListProjects {
    pub organization_id: OrganizationId,
    pub access: ProjectAccess,
}

impl Query for ListProjects {
    type Output = ApplicationResult<Vec<Project>>;
}
