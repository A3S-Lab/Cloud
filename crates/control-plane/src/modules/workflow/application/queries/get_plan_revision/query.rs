use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PlanRevisionId, WorkflowGoalId};
use crate::modules::workflow::domain::PlanRevision;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetPlanRevision {
    pub organization_id: OrganizationId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
}

impl Query for GetPlanRevision {
    type Output = ApplicationResult<PlanRevision>;
}
