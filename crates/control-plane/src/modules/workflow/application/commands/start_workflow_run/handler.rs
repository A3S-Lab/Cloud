use super::StartWorkflowRun;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, WorkflowRunId};
use crate::modules::workflow::application::WorkflowRunMutationResult;
use crate::modules::workflow::domain::{
    CreateWorkflowRunWrite, IWorkflowDefinitionRepository, IWorkflowGoalRepository,
    IWorkflowRunRepository, WorkflowRunCompiler, WorkflowRunRecord, WorkflowRunRequested,
    workflow_run_timeout_seconds,
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
            if !command.access.project_is_visible(command.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
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
                    )));
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
                    )));
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
                    )));
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
            match runs.replay(&idempotency).await {
                Ok(Some(record)) => {
                    return Ok(Ok(WorkflowRunMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{
        OrganizationId, PlanRevisionId, PrincipalId, ProjectId, WorkflowGoalId,
    };
    use crate::modules::workflow::application::{WorkflowAccess, WorkflowAccessScope};
    use crate::modules::workflow::{
        InMemoryWorkflowDefinitionRepository, InMemoryWorkflowGoalRepository,
        InMemoryWorkflowRunRepository,
    };
    use a3s_boot::ModuleRef;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn start_workflow_run_fails_closed_before_goal_lookup_in_an_ungranted_project() {
        let handler = StartWorkflowRunHandler::new(
            Arc::new(InMemoryWorkflowGoalRepository::new()),
            Arc::new(InMemoryWorkflowDefinitionRepository::new()),
            Arc::new(InMemoryWorkflowRunRepository::new()),
        );
        let result = handler
            .execute(
                StartWorkflowRun {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: WorkflowAccess::restricted([WorkflowAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    workflow_goal_id: WorkflowGoalId::new(),
                    plan_revision_id: PlanRevisionId::new(),
                    timeout_seconds: Some(30),
                    actor_principal_id: PrincipalId::new(),
                    idempotency_key: "deny-start".into(),
                    request_id: Uuid::now_v7(),
                    requested_at: Utc::now(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert_eq!(
            result,
            Err(ApplicationError::NotFound("project not found".into()))
        );
    }
}
