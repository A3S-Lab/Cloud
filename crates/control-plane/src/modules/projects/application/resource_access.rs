use crate::modules::projects::domain::entities::Project;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Projects visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments for environment listing.
/// Environment selectors expose the parent project only for collection
/// navigation; they do not authorize project-level attribution reads. Node
/// selectors have no ownership meaning for Projects and are discarded by the
/// root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProjectAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ProjectAccessScope {
    fn project_id(self) -> ProjectId {
        match self {
            Self::Project { project_id } | Self::Environment { project_id, .. } => project_id,
        }
    }

    fn authorizes_project(self, project_id: ProjectId) -> bool {
        matches!(
            self,
            Self::Project {
                project_id: granted
            } if granted == project_id
        )
    }

    fn allows_environment(self, project_id: ProjectId, environment_id: EnvironmentId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
            Self::Environment {
                project_id: granted_project,
                environment_id: granted_environment,
            } => granted_project == project_id && granted_environment == environment_id,
        }
    }
}

/// Projects-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value so Projects
/// Application never imports Identity grant vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<ProjectAccessScope>,
}

impl ProjectAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = ProjectAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = ProjectAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    /// Organization-catalog mutation (create project) requires organization-wide
    /// visibility. Restricted project/environment grants fail closed here.
    pub(crate) const fn organization_catalog_is_visible(&self) -> bool {
        self.organization_wide
    }

    /// Parent project appears in list/navigation when any project or environment
    /// grant mentions it. This does not authorize project-level mutation/read.
    pub(crate) fn project_is_visible_in_collection(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.project_id() == project_id)
    }

    /// Exact project authority required for attribution and other project-owned
    /// aggregates. Environment-only grants fail closed here.
    pub(crate) fn project_is_authorized(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.authorizes_project(project_id))
    }

    pub(crate) fn environment_is_visible(
        &self,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows_environment(project_id, environment_id))
    }
}

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
        access: &ProjectAccess,
    ) -> ApplicationResult<Project> {
        let project = match self.projects.find(organization_id, project_id).await {
            Ok(Some(project)) => project,
            Ok(None) | Err(RepositoryError::NotFound) => return Err(project_not_found()),
            Err(error) => return Err(error.into()),
        };
        if !access.project_is_authorized(project_id) {
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
    use crate::modules::projects::domain::events::ProjectCreated;
    use crate::modules::projects::domain::value_objects::ProjectName;
    use crate::modules::projects::infrastructure::persistence::InMemoryProjectsRepository;
    use crate::modules::shared_kernel::domain::IdempotencyRequest;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn environment_grants_navigate_collections_without_project_authority() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = ProjectAccess::restricted([ProjectAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(access.project_is_visible_in_collection(project_id));
        assert!(!access.project_is_authorized(project_id));
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
    }

    #[test]
    fn project_grants_include_descendant_environments() {
        let project_id = ProjectId::new();
        let access = ProjectAccess::restricted([ProjectAccessScope::Project { project_id }]);
        assert!(access.project_is_visible_in_collection(project_id));
        assert!(access.project_is_authorized(project_id));
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
    }

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
                &ProjectAccess::restricted([ProjectAccessScope::Environment {
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
