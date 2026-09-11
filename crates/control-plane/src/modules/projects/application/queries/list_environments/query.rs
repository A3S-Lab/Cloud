use crate::modules::projects::domain::entities::Environment;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_boot::Query;
use crate::modules::projects::application::ProjectAccess;

#[derive(Debug, Clone)]
pub struct ListEnvironments {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub access: ProjectAccess,
}

impl Query for ListEnvironments {
    type Output = ApplicationResult<Vec<Environment>>;
}
