use crate::modules::artifacts::domain::BuildRun;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetBuildRun {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
}

impl Query for GetBuildRun {
    type Output = ApplicationResult<BuildRun>;
}
