use super::{CreateProject, CreateProjectResult};
use crate::modules::identity::domain::repositories::IOrganizationRepository;
use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::events::ProjectCreated;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::projects::domain::value_objects::ProjectName;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, ProjectId};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::Utc;
use std::sync::Arc;

pub struct CreateProjectHandler {
    organizations: Arc<dyn IOrganizationRepository>,
    projects: Arc<dyn IProjectRepository>,
}

impl CreateProjectHandler {
    pub fn new(
        organizations: Arc<dyn IOrganizationRepository>,
        projects: Arc<dyn IProjectRepository>,
    ) -> Self {
        Self {
            organizations,
            projects,
        }
    }
}

impl CommandHandler<CreateProject> for CreateProjectHandler {
    fn execute(
        &self,
        command: CreateProject,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CreateProjectResult>>>
    {
        let organizations = Arc::clone(&self.organizations);
        let projects = Arc::clone(&self.projects);
        Box::pin(async move {
            match organizations.find(command.organization_id).await {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "organization not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            let name = match ProjectName::parse(command.name) {
                Ok(name) => name,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organization_id": command.organization_id,
                "name": name.as_str(),
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!("organizations/{}/projects", command.organization_id),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let project =
                Project::create(command.organization_id, ProjectId::new(), name, Utc::now());
            let event = ProjectCreated::envelope(&project, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            let result = match projects.create(project, event, idempotency).await {
                Ok(result) => result,
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(Ok(CreateProjectResult {
                project: result.value,
                replayed: result.replayed,
            }))
        })
    }
}
