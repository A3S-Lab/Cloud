use super::CreateWorkflowGoal;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    IdempotencyRequest, PlanRevisionId, RepositoryError, WorkflowGoalId,
};
use crate::modules::workflow::application::{
    IWorkflowEnvironmentAccess, IWorkflowProjectAccess, WorkflowEnvironmentScope,
    WorkflowGoalMutationResult, WorkflowProjectScope,
};
use crate::modules::workflow::domain::{
    CreateWorkflowGoalWrite, IOntologyRepository, IWorkflowDefinitionRepository,
    IWorkflowGoalRepository, WorkflowGoalCompiled, WorkflowGoalContract, WorkflowGoalRecord,
    WorkflowPlanCompiler,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateWorkflowGoalHandler {
    projects: Arc<dyn IWorkflowProjectAccess>,
    environments: Arc<dyn IWorkflowEnvironmentAccess>,
    workflows: Arc<dyn IWorkflowDefinitionRepository>,
    ontologies: Arc<dyn IOntologyRepository>,
    goals: Arc<dyn IWorkflowGoalRepository>,
}

impl CreateWorkflowGoalHandler {
    pub fn new(
        projects: Arc<dyn IWorkflowProjectAccess>,
        environments: Arc<dyn IWorkflowEnvironmentAccess>,
        workflows: Arc<dyn IWorkflowDefinitionRepository>,
        ontologies: Arc<dyn IOntologyRepository>,
        goals: Arc<dyn IWorkflowGoalRepository>,
    ) -> Self {
        Self {
            projects,
            environments,
            workflows,
            ontologies,
            goals,
        }
    }
}

impl CommandHandler<CreateWorkflowGoal> for CreateWorkflowGoalHandler {
    fn execute(
        &self,
        command: CreateWorkflowGoal,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowGoalMutationResult>>>
    {
        let projects = Arc::clone(&self.projects);
        let environments = Arc::clone(&self.environments);
        let workflows = Arc::clone(&self.workflows);
        let ontologies = Arc::clone(&self.ontologies);
        let goals = Arc::clone(&self.goals);
        Box::pin(async move {
            if !command.access.project_is_visible(command.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            let scope = match WorkflowProjectScope::new(command.organization_id, command.project_id)
            {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match projects.project_exists(scope).await {
                Ok(true) => {}
                Ok(false) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound("project not found".into())));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let contract = match WorkflowGoalContract::parse_acl(&command.goal_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let spec = contract.spec();
            if let Some(environment_id) = spec.environment_id {
                let environment_scope = match WorkflowEnvironmentScope::new(
                    command.organization_id,
                    command.project_id,
                    environment_id,
                ) {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
                match environments.environment_exists(environment_scope).await {
                    Ok(true) => {}
                    Ok(false) | Err(RepositoryError::NotFound) => {
                        return Ok(Err(ApplicationError::NotFound(
                            "environment not found in project".into(),
                        )));
                    }
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            let definition = match workflows
                .find(command.organization_id, spec.workflow_definition_id)
                .await
            {
                Ok(Some(value)) if value.project_id == command.project_id => value,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "WorkflowDefinition not found in project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let workflow_revision = match workflows
                .find_revision(
                    command.organization_id,
                    spec.workflow_definition_id,
                    spec.workflow_revision_id,
                )
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Workflow revision not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let ontology_revision = match ontologies
                .find_revision(
                    command.organization_id,
                    spec.ontology_id,
                    spec.ontology_revision_id,
                )
                .await
            {
                Ok(Some(value)) if value.project_id == command.project_id => value,
                Ok(Some(_)) | Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Ontology revision not found in project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "goalDigest": contract.digest(),
                "inputDigest": contract.input_digest(),
                "compilerRevision": WorkflowPlanCompiler::compiler_revision(&workflow_revision),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/workflow-goals",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match goals.replay(&idempotency).await {
                Ok(Some(record)) => {
                    return Ok(Ok(WorkflowGoalMutationResult {
                        record,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let compiled = match WorkflowPlanCompiler::compile_goal(
                WorkflowGoalId::new(),
                PlanRevisionId::new(),
                contract,
                &definition,
                &workflow_revision,
                &ontology_revision,
                command.actor_principal_id,
                Utc::now(),
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = WorkflowGoalCompiled::envelope(
                &compiled.goal,
                &compiled.plan_revision,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match goals
                .create(CreateWorkflowGoalWrite {
                    record: WorkflowGoalRecord {
                        goal: compiled.goal,
                        plan_revision: compiled.plan_revision,
                    },
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
            Ok(Ok(WorkflowGoalMutationResult {
                record: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, ProjectId};
    use crate::modules::workflow::application::{WorkflowAccess, WorkflowAccessScope};
    use crate::modules::workflow::{
        InMemoryOntologyRepository, InMemoryWorkflowDefinitionRepository,
        InMemoryWorkflowGoalRepository,
    };
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use uuid::Uuid;

    struct AllowProject;

    #[async_trait]
    impl IWorkflowProjectAccess for AllowProject {
        async fn project_exists(
            &self,
            _scope: WorkflowProjectScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    struct AllowEnvironment;

    #[async_trait]
    impl IWorkflowEnvironmentAccess for AllowEnvironment {
        async fn environment_exists(
            &self,
            _scope: WorkflowEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn create_workflow_goal_fails_closed_before_project_lookup_in_an_ungranted_project() {
        let handler = CreateWorkflowGoalHandler::new(
            Arc::new(AllowProject),
            Arc::new(AllowEnvironment),
            Arc::new(InMemoryWorkflowDefinitionRepository::new()),
            Arc::new(InMemoryOntologyRepository::new()),
            Arc::new(InMemoryWorkflowGoalRepository::new()),
        );
        let result = handler
            .execute(
                CreateWorkflowGoal {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: WorkflowAccess::restricted([WorkflowAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    goal_acl: "goal {}".into(),
                    actor_principal_id: PrincipalId::new(),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
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
