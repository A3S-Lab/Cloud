use crate::modules::artifacts::application::BuildRunLogPage;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId};
use a3s_boot::Query;
use a3s_runtime::contract::RuntimeLogStream;

#[derive(Debug, Clone)]
pub struct GetBuildRunLogs {
    pub organization_id: OrganizationId,
    pub build_run_id: BuildRunId,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<RuntimeLogStream>,
}

impl Query for GetBuildRunLogs {
    type Output = ApplicationResult<BuildRunLogPage>;
}
