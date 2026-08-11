use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::WorkflowGoalRecord;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListWorkflowGoals {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

impl Query for ListWorkflowGoals {
    type Output = ApplicationResult<Vec<WorkflowGoalRecord>>;
}
