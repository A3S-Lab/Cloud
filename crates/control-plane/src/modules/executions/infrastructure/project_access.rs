use crate::modules::executions::application::{ExecutionsProjectScope, IExecutionsProjectAccess};
use crate::modules::projects::domain::repositories::{
    IProjectRepository, ProjectAttributionRecord, UpdateProjectAttributionWrite,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Projects project authority.
#[derive(Clone)]
pub struct ProjectsExecutionsProjectAccessAdapter {
    projects: Arc<dyn IProjectRepository>,
}

impl ProjectsExecutionsProjectAccessAdapter {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }
}

#[async_trait]
impl IExecutionsProjectAccess for ProjectsExecutionsProjectAccessAdapter {
    async fn project_exists(&self, scope: ExecutionsProjectScope) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .projects
            .find(scope.organization_id(), scope.project_id())
            .await?
        {
            Some(project)
                if project.organization_id == scope.organization_id()
                    && project.id == scope.project_id()
                    && project.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Projects returned inconsistent Executions project evidence".into(),
            )),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::projects::domain::entities::{Project, ProjectAttributionProfile};
    use crate::modules::projects::domain::value_objects::ProjectName;
    use crate::modules::shared_kernel::domain::{
        IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectAttributionProfileId, ProjectId,
    };
    use a3s_cloud_contracts::DomainEventEnvelope;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubProjectRepository {
        project: Option<Project>,
        find_calls: AtomicUsize,
    }

    #[async_trait]
    impl IProjectRepository for StubProjectRepository {
        async fn create(
            &self,
            _project: Project,
            _event: DomainEventEnvelope,
            _idempotency: IdempotencyRequest,
        ) -> Result<IdempotentWrite<Project>, RepositoryError> {
            unreachable!("project access adapter never creates projects")
        }

        async fn find(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
        ) -> Result<Option<Project>, RepositoryError> {
            self.find_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.project.clone())
        }

        async fn list(
            &self,
            _organization_id: OrganizationId,
        ) -> Result<Vec<Project>, RepositoryError> {
            unreachable!("project access adapter never lists projects")
        }

        async fn replay_attribution_update(
            &self,
            _idempotency: &IdempotencyRequest,
        ) -> Result<Option<IdempotentWrite<ProjectAttributionRecord>>, RepositoryError> {
            unreachable!("project access adapter never replays attribution")
        }

        async fn update_attribution(
            &self,
            _write: UpdateProjectAttributionWrite,
        ) -> Result<IdempotentWrite<ProjectAttributionRecord>, RepositoryError> {
            unreachable!("project access adapter never updates attribution")
        }

        async fn find_attribution_profile(
            &self,
            _organization_id: OrganizationId,
            _project_id: ProjectId,
            _attribution_profile_id: ProjectAttributionProfileId,
        ) -> Result<Option<ProjectAttributionProfile>, RepositoryError> {
            unreachable!("project access adapter never loads attribution profiles")
        }
    }

    #[tokio::test]
    async fn adapter_projects_only_exact_existing_project_evidence() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let project = Project::create(
            organization_id,
            project_id,
            ProjectName::parse("demo").expect("project name"),
            Utc::now(),
        );
        let present =
            ProjectsExecutionsProjectAccessAdapter::new(Arc::new(StubProjectRepository {
                project: Some(project),
                find_calls: AtomicUsize::new(0),
            }));
        let scope = ExecutionsProjectScope::new(organization_id, project_id).unwrap();
        assert!(present.project_exists(scope).await.unwrap());

        let missing =
            ProjectsExecutionsProjectAccessAdapter::new(Arc::new(StubProjectRepository {
                project: None,
                find_calls: AtomicUsize::new(0),
            }));
        assert!(!missing.project_exists(scope).await.unwrap());
    }
}
