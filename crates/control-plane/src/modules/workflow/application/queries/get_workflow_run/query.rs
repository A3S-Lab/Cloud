use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunRecord;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowRun {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
}

impl Query for GetWorkflowRun {
    type Output = ApplicationResult<WorkflowRunRecord>;
}
