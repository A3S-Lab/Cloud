use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunDiagnostics;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetWorkflowRunDiagnostics {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowRunDiagnostics {
    type Output = ApplicationResult<WorkflowRunDiagnostics>;
}
