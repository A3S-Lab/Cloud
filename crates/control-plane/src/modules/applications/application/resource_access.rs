use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Applications visibility selector projected from an Identity decision.
///
/// Project selectors authorize project-owned application aggregates and cover
/// every environment under that project. Environment selectors authorize only
/// the exact environment for invocation admission. Node selectors have no
/// ownership meaning here and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApplicationAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ApplicationAccessScope {
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

/// Applications-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value so Applications
/// Application never imports Identity grant vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<ApplicationAccessScope>,
}

impl ApplicationAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = ApplicationAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = ApplicationAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    /// Exact project authority required for application aggregates and delivery.
    /// Environment-only grants fail closed here.
    pub(crate) fn project_is_authorized(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.authorizes_project(project_id))
    }

    /// Project grants cover all descendant environments; environment grants are exact.
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

pub(super) fn project(project_id: ProjectId, access: &ApplicationAccess) -> ApplicationResult<()> {
    if access.project_is_authorized(project_id) {
        return Ok(());
    }
    Err(project_not_found())
}

pub(super) fn project_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application project not found".into())
}

pub(super) fn environment(
    project_id: ProjectId,
    environment_id: EnvironmentId,
    access: &ApplicationAccess,
) -> ApplicationResult<()> {
    if access.environment_is_visible(project_id, environment_id) {
        return Ok(());
    }
    Err(ApplicationError::NotFound(
        "Application environment not found".into(),
    ))
}

pub(super) fn application_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application not found".into())
}

pub(super) fn release_not_found() -> ApplicationError {
    ApplicationError::NotFound("Application release not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_authorizes_projects_and_environments() {
        let access = ApplicationAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.project_is_authorized(ProjectId::new()));
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn project_grant_covers_environments_but_environment_grant_does_not_authorize_project() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project_access =
            ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]);
        assert!(project_access.project_is_authorized(project_id));
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.project_is_authorized(ProjectId::new()));

        let environment_access =
            ApplicationAccess::restricted([ApplicationAccessScope::Environment {
                project_id,
                environment_id,
            }]);
        assert!(!environment_access.project_is_authorized(project_id));
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
