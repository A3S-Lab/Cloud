use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowGoalId};
use crate::modules::workflow::domain::WorkflowGoalRecord;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetWorkflowGoal {
    pub organization_id: OrganizationId,
    pub workflow_goal_id: WorkflowGoalId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowGoal {
    type Output = ApplicationResult<WorkflowGoalRecord>;
}
