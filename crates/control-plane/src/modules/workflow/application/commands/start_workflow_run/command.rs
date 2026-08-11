use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OrganizationId, PrincipalId, ProjectId, WorkflowGoalId,
};
use crate::modules::workflow::application::WorkflowRunMutationResult;
use a3s_boot::Command;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StartWorkflowRun {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workflow_goal_id: WorkflowGoalId,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for StartWorkflowRun {
    type Output = ApplicationResult<WorkflowRunMutationResult>;
}
