use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::domain::ExternalSourceRevision;
use a3s_boot::Query;

#[derive(Debug, Clone, Copy)]
pub struct ListSourceRevisions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListSourceRevisions {
    type Output = ApplicationResult<Vec<ExternalSourceRevision>>;
}
