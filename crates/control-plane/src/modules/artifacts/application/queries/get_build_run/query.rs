use crate::modules::artifacts::domain::BuildRun;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetBuildRun {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetBuildRun {
    type Output = ApplicationResult<BuildRun>;
}
