use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunHistoryPage;
use a3s_boot::Query;

pub const WORKFLOW_RUN_HISTORY_MAX_LIMIT: usize = 100;

#[derive(Debug, Clone)]
pub struct GetWorkflowRunHistory {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub resource_access: ResourceAccessEvaluator,
    pub after_sequence: u64,
    pub limit: usize,
}

impl Query for GetWorkflowRunHistory {
    type Output = ApplicationResult<WorkflowRunHistoryPage>;
}
