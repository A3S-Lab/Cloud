use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunRecord;
use a3s_boot::Query;
use std::time::Duration;

pub const WORKFLOW_RUN_WAIT_MAX_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct WaitWorkflowRun {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub timeout: Duration,
}

impl Query for WaitWorkflowRun {
    type Output = ApplicationResult<WorkflowRunRecord>;
}
