use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OperationId, OrganizationId, PlanRevisionId, PrincipalId, ProjectId,
    Sha256Digest, WorkflowGoalId, WorkflowRunId,
};
use crate::modules::workflow::domain::{PlanRevision, WorkflowGoal};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: WorkflowRunId,
    pub workflow_goal_id: WorkflowGoalId,
    pub plan_revision_id: PlanRevisionId,
    pub plan_digest: Sha256Digest,
    pub operation_id: OperationId,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
}

impl WorkflowRun {
    pub fn create(
        id: WorkflowRunId,
        goal: &WorkflowGoal,
        plan: &PlanRevision,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id: goal.organization_id,
            project_id: goal.project_id,
            id,
            workflow_goal_id: goal.id,
            plan_revision_id: plan.id,
            plan_digest: plan.digest.clone(),
            operation_id: OperationId::from_uuid(id.as_uuid()),
            requested_by,
            requested_at: canonical_timestamp(requested_at),
        };
        value.validate_against(goal, plan)?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: WorkflowRunId,
        workflow_goal_id: WorkflowGoalId,
        plan_revision_id: PlanRevisionId,
        plan_digest: Sha256Digest,
        operation_id: OperationId,
        requested_by: PrincipalId,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            id,
            workflow_goal_id,
            plan_revision_id,
            plan_digest,
            operation_id,
            requested_by,
            requested_at: canonical_timestamp(requested_at),
        };
        value.validate_identity()?;
        Ok(value)
    }

    pub fn validate_against(&self, goal: &WorkflowGoal, plan: &PlanRevision) -> Result<(), String> {
        self.validate_identity()?;
        goal.validate(plan)?;
        if goal.organization_id != self.organization_id
            || goal.project_id != self.project_id
            || goal.id != self.workflow_goal_id
            || goal.plan_revision_id != self.plan_revision_id
            || goal.plan_digest != self.plan_digest
            || plan.organization_id != self.organization_id
            || plan.project_id != self.project_id
            || plan.workflow_goal_id != self.workflow_goal_id
            || plan.id != self.plan_revision_id
            || plan.digest != self.plan_digest
        {
            return Err("WorkflowRun does not bind the exact Goal and PlanRevision".into());
        }
        Ok(())
    }

    pub fn validate_identity(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.workflow_goal_id.as_uuid().is_nil()
            || self.plan_revision_id.as_uuid().is_nil()
            || self.requested_by.as_uuid().is_nil()
            || self.operation_id.as_uuid() != self.id.as_uuid()
        {
            return Err("WorkflowRun identity or Operation correlation is invalid".into());
        }
        Ok(())
    }
}
