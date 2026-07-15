use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::workloads::application::queries::WorkloadQueryResult;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListWorkloads {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListWorkloads {
    type Output = ApplicationResult<Vec<WorkloadQueryResult>>;
}
