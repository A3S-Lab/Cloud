use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
use crate::modules::workflow::application::WorkflowGoalMutationResult;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateWorkflowGoal {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub goal_acl: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CreateWorkflowGoal {
    type Output = ApplicationResult<WorkflowGoalMutationResult>;
}
