use crate::modules::artifacts::domain::BuildEvidence;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetBuildEvidence {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
}

impl Query for GetBuildEvidence {
    type Output = ApplicationResult<BuildEvidence>;
}
