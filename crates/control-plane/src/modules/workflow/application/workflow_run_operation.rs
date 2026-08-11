use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::workflow::domain::WorkflowRun;
use serde::{Deserialize, Serialize};

pub const WORKFLOW_RUN_FLOW_NAME: &str = "cloud.workflow.run";
pub const WORKFLOW_RUN_FLOW_VERSION: &str = "1";
pub const WORKFLOW_RUN_INPUT_SCHEMA: &str = "cloud.workflow.run-input.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowRunOperationInput {
    pub schema: String,
    pub organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    pub project_id: crate::modules::shared_kernel::domain::ProjectId,
    pub workflow_run_id: crate::modules::shared_kernel::domain::WorkflowRunId,
    pub workflow_goal_id: crate::modules::shared_kernel::domain::WorkflowGoalId,
    pub plan_revision_id: crate::modules::shared_kernel::domain::PlanRevisionId,
    pub plan_digest: String,
}

impl WorkflowRunOperationInput {
    pub fn from_run(run: &WorkflowRun) -> Self {
        Self {
            schema: WORKFLOW_RUN_INPUT_SCHEMA.into(),
            organization_id: run.organization_id,
            project_id: run.project_id,
            workflow_run_id: run.id,
            workflow_goal_id: run.workflow_goal_id,
            plan_revision_id: run.plan_revision_id,
            plan_digest: run.plan_digest.to_string(),
        }
    }

    pub fn validate_against(&self, run: &WorkflowRun) -> Result<(), String> {
        if self.schema != WORKFLOW_RUN_INPUT_SCHEMA
            || self.organization_id != run.organization_id
            || self.project_id != run.project_id
            || self.workflow_run_id != run.id
            || self.workflow_goal_id != run.workflow_goal_id
            || self.plan_revision_id != run.plan_revision_id
            || self.plan_digest != run.plan_digest.as_str()
        {
            return Err("WorkflowRun Operation input does not match its immutable run".into());
        }
        Ok(())
    }
}

pub fn workflow_run_operation(run: &WorkflowRun) -> Result<OperationRequest, String> {
    run.validate_identity()?;
    Ok(OperationRequest::new(
        run.operation_id,
        run.organization_id,
        OperationSubject::new("workflow_run", run.id.as_uuid())?,
        WorkflowIdentity::new(WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION)?,
        serde_json::to_value(WorkflowRunOperationInput::from_run(run))
            .map_err(|error| format!("could not encode WorkflowRun Operation input: {error}"))?,
        run.requested_at,
    ))
}
