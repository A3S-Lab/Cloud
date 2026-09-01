use crate::modules::workloads::WorkloadAccess;
use crate::presentation::{resource_access_evaluator, workload_access as project_workload_access};
use a3s_boot::{BootRequest, Result};

/// Projects authenticated Identity authority once into the vocabulary owned
/// by Workloads at the inbound adapter boundary.
pub(super) fn workload_access(request: &BootRequest) -> Result<WorkloadAccess> {
    Ok(project_workload_access(&resource_access_evaluator(
        &request.require_auth_principal()?,
    )?))
}
