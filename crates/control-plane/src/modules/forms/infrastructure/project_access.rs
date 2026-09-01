use crate::modules::forms::application::{FormProjectScope, IFormProjectAccess};
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Projects ownership authority.
///
/// The adapter validates exact owner evidence, then discards the Project
/// aggregate so Forms cannot acquire a second Project model or repository.
#[derive(Clone)]
pub struct ProjectsFormProjectAccessAdapter {
    projects: Arc<dyn IProjectRepository>,
}

impl ProjectsFormProjectAccessAdapter {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }
}

#[async_trait]
impl IFormProjectAccess for ProjectsFormProjectAccessAdapter {
    async fn project_exists(&self, scope: FormProjectScope) -> Result<bool, RepositoryError> {
        match self
            .projects
            .find(scope.organization_id, scope.project_id)
            .await?
        {
            Some(project)
                if project.organization_id == scope.organization_id
                    && project.id == scope.project_id
                    && project.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Projects returned inconsistent Forms project evidence".into(),
            )),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::forms::application::IFormProjectAccess;
    use crate::modules::projects::domain::entities::Project;
    use crate::modules::projects::domain::events::ProjectCreated;
    use crate::modules::projects::domain::value_objects::ProjectName;
    use crate::modules::projects::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::{IdempotencyRequest, OrganizationId, ProjectId};
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn adapter_projects_only_exact_existing_owner_evidence() {
        let projects = Arc::new(InMemoryProjectsRepository::new());
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let project = Project::create(
            organization_id,
            project_id,
            ProjectName::parse("Forms").expect("project name"),
            Utc::now(),
        );
        projects
            .create(
                project.clone(),
                ProjectCreated::envelope(&project, Uuid::now_v7()).expect("project event"),
                IdempotencyRequest::new("projects", "forms-owner", b"forms-owner")
                    .expect("idempotency"),
            )
            .await
            .expect("create project");
        let adapter = ProjectsFormProjectAccessAdapter::new(projects);

        assert!(adapter
            .project_exists(FormProjectScope {
                organization_id,
                project_id,
            })
            .await
            .expect("existing project evidence"));
        assert!(!adapter
            .project_exists(FormProjectScope {
                organization_id,
                project_id: ProjectId::new(),
            })
            .await
            .expect("missing project evidence"));
        assert!(!adapter
            .project_exists(FormProjectScope {
                organization_id: OrganizationId::new(),
                project_id,
            })
            .await
            .expect("foreign organization evidence"));
    }
}
