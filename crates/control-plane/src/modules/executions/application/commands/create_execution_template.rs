use crate::modules::executions::application::{
    ExecutionAccess, ExecutionsProjectScope, IExecutionsProjectAccess,
};
use crate::modules::executions::domain::events::ExecutionTemplatePublished;
use crate::modules::executions::domain::{
    CreateExecutionTemplateRevision, ExecutionTemplateDefinition, ExecutionTemplateRevision,
    IExecutionTemplateRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ExecutionTemplateId, ExecutionTemplateRevisionId, IdempotencyRequest, OrganizationId,
    PrincipalId, ProjectId,
};
use a3s_boot::{BootError, Command, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateExecutionTemplateCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub access: ExecutionAccess,
    pub definition_acl: String,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateExecutionTemplateCommand {
    type Output = ApplicationResult<CreateExecutionTemplateResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateExecutionTemplateResult {
    pub revision: ExecutionTemplateRevision,
    pub replayed: bool,
}

pub struct CreateExecutionTemplateHandler {
    projects: Arc<dyn IExecutionsProjectAccess>,
    templates: Arc<dyn IExecutionTemplateRepository>,
}

impl CreateExecutionTemplateHandler {
    pub fn new(
        projects: Arc<dyn IExecutionsProjectAccess>,
        templates: Arc<dyn IExecutionTemplateRepository>,
    ) -> Self {
        Self {
            projects,
            templates,
        }
    }
}

impl CommandHandler<CreateExecutionTemplateCommand> for CreateExecutionTemplateHandler {
    fn execute(
        &self,
        command: CreateExecutionTemplateCommand,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<CreateExecutionTemplateResult>>,
    > {
        let projects = Arc::clone(&self.projects);
        let templates = Arc::clone(&self.templates);
        Box::pin(async move {
            if !command.access.project_is_visible(command.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            let project_scope =
                match ExecutionsProjectScope::new(command.organization_id, command.project_id) {
                    Ok(scope) => scope,
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                };
            match projects.project_exists(project_scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound("project not found".into())));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let definition = match ExecutionTemplateDefinition::parse_acl(&command.definition_acl) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "projectId": command.project_id,
                "definitionAcl": definition.canonical_acl(),
                "definitionDigest": definition.digest(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/execution-templates",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match templates.replay_create(&idempotency).await {
                Ok(Some(replay)) => {
                    return Ok(Ok(CreateExecutionTemplateResult {
                        revision: replay.value,
                        replayed: true,
                    }));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let revision = match ExecutionTemplateRevision::create(
                command.organization_id,
                command.project_id,
                ExecutionTemplateId::new(),
                ExecutionTemplateRevisionId::new(),
                definition,
                command.actor_principal_id,
                command.requested_at,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let event = ExecutionTemplatePublished::envelope(&revision, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match templates
                .create(CreateExecutionTemplateRevision {
                    revision,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(CreateExecutionTemplateResult {
                    revision: write.value,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::executions::application::resource_access::{
        ExecutionAccess, ExecutionAccessScope,
    };
    use crate::modules::executions::infrastructure::InMemoryExecutionTemplateRepository;
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;

    struct AllowAllProjects;

    #[async_trait]
    impl IExecutionsProjectAccess for AllowAllProjects {
        async fn project_exists(
            &self,
            _scope: ExecutionsProjectScope,
        ) -> Result<bool, crate::modules::shared_kernel::domain::RepositoryError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn restricted_command_fails_closed_before_creating_in_an_ungranted_project() {
        let handler = CreateExecutionTemplateHandler::new(
            Arc::new(AllowAllProjects),
            Arc::new(InMemoryExecutionTemplateRepository::new()),
        );
        let result = handler
            .execute(
                CreateExecutionTemplateCommand {
                    organization_id: OrganizationId::new(),
                    project_id: ProjectId::new(),
                    access: ExecutionAccess::restricted([ExecutionAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    definition_acl: "agent {}".into(),
                    actor_principal_id: PrincipalId::new(),
                    idempotency_key: "k1".into(),
                    request_id: Uuid::new_v4(),
                    requested_at: Utc::now(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "project not found"
        ));
    }
}
