use crate::modules::artifacts::domain::BuildRun;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListBuildRuns {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub limit: usize,
}

impl Query for ListBuildRuns {
    type Output = ApplicationResult<Vec<BuildRun>>;
}
