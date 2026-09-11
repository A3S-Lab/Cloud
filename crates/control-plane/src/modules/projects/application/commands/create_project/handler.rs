use super::{CreateProject, CreateProjectResult};
use crate::modules::projects::application::IProjectOrganizationAccess;
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
    organizations: Arc<dyn IProjectOrganizationAccess>,
    projects: Arc<dyn IProjectRepository>,
}

impl CreateProjectHandler {
    pub fn new(
        organizations: Arc<dyn IProjectOrganizationAccess>,
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
            if !command.access.organization_catalog_is_visible() {
                return Ok(Err(ApplicationError::NotFound(
                    "organization not found".into(),
                )));
            }
            if let Err(error) = organizations
                .require_organization(command.organization_id)
                .await
            {
                return Ok(Err(error));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::projects::application::{ProjectAccess, ProjectAccessScope};
    use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
    use a3s_boot::ModuleRef;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use uuid::Uuid;

    struct TrackingOrganizationAccess {
        called: AtomicBool,
    }

    #[async_trait]
    impl IProjectOrganizationAccess for TrackingOrganizationAccess {
        async fn require_organization(
            &self,
            _organization_id: OrganizationId,
        ) -> ApplicationResult<()> {
            self.called.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn create_project_fails_closed_before_organization_lookup_without_catalog_visibility() {
        let organizations = Arc::new(TrackingOrganizationAccess {
            called: AtomicBool::new(false),
        });
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let handler = CreateProjectHandler::new(
            Arc::clone(&organizations) as Arc<dyn IProjectOrganizationAccess>,
            Arc::clone(&projects) as Arc<dyn IProjectRepository>,
        );
        let result = handler
            .execute(
                CreateProject {
                    organization_id: OrganizationId::new(),
                    access: ProjectAccess::restricted([ProjectAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                    name: "denied-project".into(),
                    idempotency_key: "deny-create".into(),
                    request_id: Uuid::now_v7(),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message)) if message == "organization not found"
        ));
        assert!(!organizations.called.load(Ordering::SeqCst));
    }
}
