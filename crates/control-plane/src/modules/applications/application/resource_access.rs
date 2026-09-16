use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{ApplicationId, EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Applications visibility selector projected from an Identity decision.
///
/// Project selectors authorize project-owned application aggregates and cover
/// every environment under that project. Environment selectors authorize only
/// the exact environment for invocation admission. Application selectors
/// authorize only the exact published application (APP0.3 Rule 8) and never
/// imply project-wide management authority. Node selectors have no ownership
/// meaning here and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApplicationAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
    Application {
        project_id: ProjectId,
        application_id: ApplicationId,
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
            Self::Application { .. } => false,
        }
    }

    fn allows_application(self, project_id: ProjectId, application_id: ApplicationId) -> bool {
        match self {
            Self::Project {
                project_id: granted,
            } => granted == project_id,
            Self::Application {
                project_id: granted_project,
                application_id: granted_application,
            } => granted_project == project_id && granted_application == application_id,
            Self::Environment { .. } => false,
        }
    }

    fn allows_exact_application(self, project_id: ProjectId, application_id: ApplicationId) -> bool {
        matches!(
            self,
            Self::Application {
                project_id: granted_project,
                application_id: granted_application,
            } if granted_project == project_id && granted_application == application_id
        )
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
    /// Environment-only and Application-only grants fail closed here.
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

    /// Project grants cover applications in the project; Application grants are exact.
    pub(crate) fn application_is_visible(
        &self,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows_application(project_id, application_id))
    }

    /// Published application delivery requires an exact Application grant (Rule 8).
    pub(crate) fn exact_application_is_authorized(
        &self,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows_exact_application(project_id, application_id))
    }
}

pub(super) fn project(project_id: ProjectId, access: &ApplicationAccess) -> ApplicationResult<()> {
    if access.project_is_authorized(project_id) {
        return Ok(());
    }
    Err(project_not_found())
}

/// Published application delivery: Project grants or exact Application grants.
///
/// Management project-member routes keep working via Project authority. Delivery
/// Rule 8 tokens with only an exact Application grant also pass here; the
/// authenticated Delivery controller still requires exact Application grant.
pub(super) fn published_application(
    project_id: ProjectId,
    application_id: ApplicationId,
    access: &ApplicationAccess,
) -> ApplicationResult<()> {
    if access.project_is_authorized(project_id)
        || access.exact_application_is_authorized(project_id, application_id)
    {
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
    fn organization_wide_authorizes_projects_environments_and_applications() {
        let access = ApplicationAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.project_is_authorized(ProjectId::new()));
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
        assert!(access.application_is_visible(ProjectId::new(), ApplicationId::new()));
        assert!(access.exact_application_is_authorized(ProjectId::new(), ApplicationId::new()));
    }

    #[test]
    fn project_grant_covers_environments_and_applications_but_not_exact_delivery_alone() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let application_id = ApplicationId::new();
        let project_access =
            ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]);
        assert!(project_access.project_is_authorized(project_id));
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(project_access.application_is_visible(project_id, application_id));
        assert!(!project_access.exact_application_is_authorized(project_id, application_id));
        assert!(!project_access.project_is_authorized(ProjectId::new()));

        let environment_access =
            ApplicationAccess::restricted([ApplicationAccessScope::Environment {
                project_id,
                environment_id,
            }]);
        assert!(!environment_access.project_is_authorized(project_id));
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.application_is_visible(project_id, application_id));

        let application_access =
            ApplicationAccess::restricted([ApplicationAccessScope::Application {
                project_id,
                application_id,
            }]);
        assert!(!application_access.project_is_authorized(project_id));
        assert!(!application_access.environment_is_visible(project_id, environment_id));
        assert!(application_access.application_is_visible(project_id, application_id));
        assert!(application_access.exact_application_is_authorized(project_id, application_id));
        assert!(!application_access.application_is_visible(project_id, ApplicationId::new()));
    }

    #[test]
    fn published_application_accepts_project_or_exact_application_grants() {
        let project_id = ProjectId::new();
        let application_id = ApplicationId::new();
        assert!(published_application(
            project_id,
            application_id,
            &ApplicationAccess::restricted([ApplicationAccessScope::Project { project_id }]),
        )
        .is_ok());
        assert!(published_application(
            project_id,
            application_id,
            &ApplicationAccess::restricted([ApplicationAccessScope::Application {
                project_id,
                application_id,
            }]),
        )
        .is_ok());
        assert!(published_application(
            project_id,
            application_id,
            &ApplicationAccess::restricted([ApplicationAccessScope::Environment {
                project_id,
                environment_id: EnvironmentId::new(),
            }]),
        )
        .is_err());
    }
}
