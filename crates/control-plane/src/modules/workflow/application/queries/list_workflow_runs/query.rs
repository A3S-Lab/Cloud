use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::WorkflowRunRecord;
use a3s_boot::Query;

pub const WORKFLOW_RUN_LIST_MAX_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct ListWorkflowRuns {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub limit: usize,
}

impl Query for ListWorkflowRuns {
    type Output = ApplicationResult<Vec<WorkflowRunRecord>>;
}
