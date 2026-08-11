use super::StartWorkflowRun;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRunId};
use crate::modules::workflow::application::{workflow_run_operation, WorkflowRunMutationResult};
use crate::modules::workflow::domain::{
    validate_locally_executable_plan, IWorkflowGoalRepository, IWorkflowRunRepository,
    StartWorkflowRunWrite, WorkflowRun, WorkflowRunRequested,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct StartWorkflowRunHandler {
    goals: Arc<dyn IWorkflowGoalRepository>,
    runs: Arc<dyn IWorkflowRunRepository>,
}

impl StartWorkflowRunHandler {
    pub fn new(
        goals: Arc<dyn IWorkflowGoalRepository>,
        runs: Arc<dyn IWorkflowRunRepository>,
    ) -> Self {
        Self { goals, runs }
    }
}

impl CommandHandler<StartWorkflowRun> for StartWorkflowRunHandler {
    fn execute(
        &self,
        command: StartWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunMutationResult>>>
    {
        let goals = Arc::clone(&self.goals);
        let runs = Arc::clone(&self.runs);
        Box::pin(async move {
            let record = match goals
                .find(command.organization_id, command.workflow_goal_id)
                .await
            {
                Ok(Some(value)) if value.goal.project_id == command.project_id => value,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "WorkflowGoal not found in project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if let Err(error) = validate_locally_executable_plan(&record.plan_revision.plan) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "workflowGoalId": record.goal.id,
                "planRevisionId": record.plan_revision.id,
                "planDigest": record.plan_revision.digest,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/workflow-runs",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let run = match WorkflowRun::create(
                WorkflowRunId::new(),
                &record.goal,
                &record.plan_revision,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let operation = workflow_run_operation(&run).map_err(BootError::Internal)?;
            let event = WorkflowRunRequested::envelope(&run, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match runs
                .start(StartWorkflowRunWrite {
                    run,
                    operation,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(WorkflowRunMutationResult {
                run: write.value,
                replayed: write.replayed,
            }))
        })
    }
}
