use crate::modules::artifacts::application::{ArtifactAccess, BuildLogStream, BuildRunLogPage};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetBuildRunLogs {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub access: ArtifactAccess,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<BuildLogStream>,
}

impl Query for GetBuildRunLogs {
    type Output = ApplicationResult<BuildRunLogPage>;
}
