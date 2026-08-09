use super::StartWorkflowRun;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRunId};
use crate::modules::workflow::application::WorkflowRunMutationResult;
use crate::modules::workflow::domain::{
    workflow_run_timeout_seconds, CreateWorkflowRunWrite, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, IWorkflowRunRepository, WorkflowRunCompiler, WorkflowRunRecord,
    WorkflowRunRequested,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct StartWorkflowRunHandler {
    goals: Arc<dyn IWorkflowGoalRepository>,
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
    runs: Arc<dyn IWorkflowRunRepository>,
}

impl StartWorkflowRunHandler {
    pub fn new(
        goals: Arc<dyn IWorkflowGoalRepository>,
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
        runs: Arc<dyn IWorkflowRunRepository>,
    ) -> Self {
        Self {
            goals,
            workflows,
            runs,
        }
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
        let workflows = Arc::clone(&self.workflows);
        let runs = Arc::clone(&self.runs);
        Box::pin(async move {
            let timeout_seconds = match workflow_run_timeout_seconds(command.timeout_seconds) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let goal_record = match goals
                .find(command.organization_id, command.workflow_goal_id)
                .await
            {
                Ok(Some(record)) if record.goal.project_id == command.project_id => record,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "WorkflowGoal not found in project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let plan_revision = match goals
                .find_plan_revision(
                    command.organization_id,
                    command.workflow_goal_id,
                    command.plan_revision_id,
                )
                .await
            {
                Ok(Some(plan)) if plan.project_id == command.project_id => plan,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "PlanRevision not found in WorkflowGoal".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let workflow_revision = match workflows
                .find_revision(
                    command.organization_id,
                    plan_revision.plan.workflow_definition_id,
                    plan_revision.plan.workflow_revision_id,
                )
                .await
            {
                Ok(Some(revision)) if revision.project_id == command.project_id => revision,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Workflow revision not found in project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "workflowGoalId": command.workflow_goal_id,
                "planRevisionId": command.plan_revision_id,
                "planDigest": plan_revision.digest,
                "timeoutSeconds": timeout_seconds,
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
            let compiled = match WorkflowRunCompiler::compile(
                WorkflowRunId::new(),
                &goal_record.goal,
                &plan_revision,
                &workflow_revision,
                Some(timeout_seconds),
                command.actor_principal_id,
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = WorkflowRunRequested::envelope(&compiled.run, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let write = match runs
                .create(CreateWorkflowRunWrite {
                    record: WorkflowRunRecord {
                        run: compiled.run,
                        steps: compiled.steps,
                    },
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => write,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(WorkflowRunMutationResult {
                record: write.value,
                replayed: write.replayed,
            }))
        })
    }
}
