use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId, WorkloadRevisionId};
use crate::modules::workloads::application::queries::WorkloadLogPage;
use a3s_boot::Query;
use a3s_runtime::contract::RuntimeLogStream;

#[derive(Debug, Clone)]
pub struct GetWorkloadLogs {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub after_sequence: Option<u64>,
    pub limit: u16,
    pub stream: Option<RuntimeLogStream>,
}

impl Query for GetWorkloadLogs {
    type Output = ApplicationResult<WorkloadLogPage>;
}
