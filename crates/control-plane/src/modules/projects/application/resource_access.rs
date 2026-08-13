use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::identity::domain::value_objects::ResourceGrantScope;
use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ProjectResourceAccess {
    projects: Arc<dyn IProjectRepository>,
}

impl ProjectResourceAccess {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }

    pub async fn project(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        evaluator: &ResourceAccessEvaluator,
    ) -> ApplicationResult<Project> {
        let project = match self.projects.find(organization_id, project_id).await {
            Ok(Some(project)) => project,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(project_not_found()),
            Err(error) => return Err(error.into()),
        };
        if !evaluator.allows(ResourceGrantScope::Project { project_id }) {
            return Err(project_not_found());
        }
        Ok(project)
    }
}

fn project_not_found() -> ApplicationError {
    ApplicationError::NotFound("project not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::services::ResourceAccessEvaluator;
    use crate::modules::projects::domain::events::ProjectCreated;
    use crate::modules::projects::domain::value_objects::ProjectName;
    use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::{EnvironmentId, IdempotencyRequest};
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn environment_only_grants_cannot_read_project_attribution() {
        let repository = Arc::new(InMemoryProjectsRepository::new());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let project = Project::create(
            organization_id,
            project_id,
            ProjectName::parse("platform").expect("name"),
            Utc::now(),
        );
        let event = ProjectCreated::envelope(&project, Uuid::now_v7()).expect("event");
        IProjectRepository::create(
            repository.as_ref(),
            project,
            event,
            IdempotencyRequest::new("test/projects", "create", b"project").expect("idempotency"),
        )
        .await
        .expect("create");

        let access: Arc<dyn IProjectRepository> = repository;
        let error = ProjectResourceAccess::new(access)
            .project(
                organization_id,
                project_id,
                &ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
                    project_id,
                    environment_id: EnvironmentId::new(),
                }]),
            )
            .await
            .expect_err("project attribution must be hidden");
        assert_eq!(
            error,
            ApplicationError::NotFound("project not found".into())
        );
    }
}
