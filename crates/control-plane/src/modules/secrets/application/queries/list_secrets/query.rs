use crate::modules::secrets::domain::Secret;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListSecrets {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListSecrets {
    type Output = ApplicationResult<Vec<Secret>>;
}
