use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::entities::Environment;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListEnvironments {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListEnvironments {
    type Output = ApplicationResult<Vec<Environment>>;
}
