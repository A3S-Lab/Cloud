use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkloadId};
use crate::modules::workloads::application::queries::WorkloadQueryResult;
use crate::modules::workloads::application::WorkloadAccess;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkload {
    pub organization_id: OrganizationId,
    pub workload_id: WorkloadId,
    pub access: WorkloadAccess,
}

impl Query for GetWorkload {
    type Output = ApplicationResult<WorkloadQueryResult>;
}
