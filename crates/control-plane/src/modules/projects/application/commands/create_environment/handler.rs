use super::{CreateEnvironment, CreateEnvironmentResult};
use crate::modules::projects::domain::entities::Environment;
use crate::modules::projects::domain::events::EnvironmentCreated;
use crate::modules::projects::domain::repositories::{IEnvironmentRepository, IProjectRepository};
use crate::modules::projects::domain::value_objects::EnvironmentName;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, IdempotencyRequest};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateEnvironmentHandler {
    projects: Arc<dyn IProjectRepository>,
    environments: Arc<dyn IEnvironmentRepository>,
}

impl CreateEnvironmentHandler {
    pub fn new(
        projects: Arc<dyn IProjectRepository>,
        environments: Arc<dyn IEnvironmentRepository>,
    ) -> Self {
        Self {
            projects,
            environments,
        }
    }
}

impl CommandHandler<CreateEnvironment> for CreateEnvironmentHandler {
    fn execute(
        &self,
        command: CreateEnvironment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateEnvironmentResult>>>
    {
        let projects = Arc::clone(&self.projects);
        let environments = Arc::clone(&self.environments);
        Box::pin(async move {
            if !command.access.project_is_authorized(command.project_id) {
                return Ok(Err(ApplicationError::NotFound(
                    "project not found in organization".into(),
                )));
            }
            match projects
                .find(command.organization_id, command.project_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "project not found in organization".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let name = match EnvironmentName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "project_id": command.project_id,
                "name": name.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments",
                    command.organization_id, command.project_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let environment = Environment::create(
                command.organization_id,
                command.project_id,
                EnvironmentId::new(),
                name,
                Utc::now(),
            );
            let event = EnvironmentCreated::envelope(&environment, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match environments.create(environment, event, idempotency).await {
                Ok(result) => result,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateEnvironmentResult {
                environment: result.value,
                replayed: result.replayed,
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::projects::application::{ProjectAccess, ProjectAccessScope};
    use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;
    use uuid::Uuid;

    #[tokio::test]
    async fn create_environment_fails_closed_before_project_lookup_without_project_authority() {
        let repository = Arc::new(InMemoryProjectsRepository::new());
        let handler = CreateEnvironmentHandler::new(
            Arc::clone(&repository) as Arc<dyn IProjectRepository>,
            Arc::clone(&repository) as Arc<dyn IEnvironmentRepository>,
        );
        let project_id = ProjectId::new();
        let result = handler
            .execute(
                CreateEnvironment {
                    organization_id: OrganizationId::new(),
                    project_id,
                    access: ProjectAccess::restricted([ProjectAccessScope::Environment {
                        project_id,
                        environment_id: EnvironmentId::new(),
                    }]),
                    name: "denied-env".into(),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "project not found in organization"
        ));
    }
}
