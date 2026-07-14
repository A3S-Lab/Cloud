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
            match projects
                .find(command.organization_id, command.project_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "project not found in organization".into(),
                    )))
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
