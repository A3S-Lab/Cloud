use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunRecord;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetWorkflowRun {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowRun {
    type Output = ApplicationResult<WorkflowRunRecord>;
}
