use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use std::collections::BTreeSet;

/// One Developer Workflows visibility selector projected from an Identity decision.
///
/// Project selectors include their descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no meaning in this
/// bounded context and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DeveloperWorkflowAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl DeveloperWorkflowAccessScope {
    fn allows(self, project_id: ProjectId, environment_id: EnvironmentId) -> bool {
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

/// Developer Workflows-owned projection of an already-authorized request.
///
/// Authentication and credential-scope enforcement remain entry-adapter
/// responsibilities. Application use cases receive only the immutable resource
/// visibility needed by this context, so they do not import Identity roles,
/// grants, evaluators, or repositories and do not repeat an Identity lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeveloperWorkflowAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<DeveloperWorkflowAccessScope>,
}

impl DeveloperWorkflowAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(
        granted_scopes: impl IntoIterator<Item = DeveloperWorkflowAccessScope>,
    ) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
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
                .any(|scope| scope.allows(project_id, environment_id))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeveloperWorkflowEnvironmentScope {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl DeveloperWorkflowEnvironmentScope {
    pub fn validate(self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
        {
            return Err("Developer Workflow environment scope is invalid".into());
        }
        Ok(())
    }
}

/// Consumer-owned read port for the Projects environment authority.
///
/// The port answers only whether the exact owner scope exists. It does not
/// interpret Identity policy or credential actions; visibility is evaluated
/// locally from `DeveloperWorkflowAccess` before this port is called.
#[async_trait]
pub trait IDeveloperWorkflowEnvironmentPort: Send + Sync {
    async fn environment_exists(
        &self,
        scope: DeveloperWorkflowEnvironmentScope,
    ) -> Result<bool, RepositoryError>;
}

pub(super) async fn authorize_environment(
    environments: &dyn IDeveloperWorkflowEnvironmentPort,
    scope: DeveloperWorkflowEnvironmentScope,
    access: &DeveloperWorkflowAccess,
) -> ApplicationResult<()> {
    scope.validate().map_err(|_| concealed_environment())?;
    if !access.environment_is_visible(scope.project_id, scope.environment_id) {
        return Err(concealed_environment());
    }
    match environments.environment_exists(scope).await {
        Ok(true) => Ok(()),
        Ok(false) | Err(RepositoryError::NotFound | RepositoryError::Forbidden(_)) => {
            Err(concealed_environment())
        }
        Err(error) => Err(error.into()),
    }
}

fn concealed_environment() -> ApplicationError {
    ApplicationError::NotFound("Developer Workflow environment not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn visibility_is_exact_project_scoped_and_canonicalized() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = DeveloperWorkflowAccessScope::Environment {
            project_id,
            environment_id,
        };
        let access = DeveloperWorkflowAccess::restricted([exact, exact]);

        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
    }

    #[test]
    fn project_visibility_includes_descendant_environments() {
        let project_id = ProjectId::new();
        let access = DeveloperWorkflowAccess::restricted([DeveloperWorkflowAccessScope::Project {
            project_id,
        }]);

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
        assert!(DeveloperWorkflowAccess::organization_wide()
            .environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[tokio::test]
    async fn invisible_scope_is_concealed_before_the_projects_port_is_called() {
        let environments = CountingEnvironmentPort::default();
        let scope = DeveloperWorkflowEnvironmentScope {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
        };

        assert!(matches!(
            authorize_environment(
                &environments,
                scope,
                &DeveloperWorkflowAccess::restricted([]),
            )
            .await,
            Err(ApplicationError::NotFound(_))
        ));
        assert_eq!(environments.calls.load(Ordering::SeqCst), 0);

        authorize_environment(
            &environments,
            scope,
            &DeveloperWorkflowAccess::organization_wide(),
        )
        .await
        .expect("visible existing environment");
        assert_eq!(environments.calls.load(Ordering::SeqCst), 1);
    }

    #[derive(Default)]
    struct CountingEnvironmentPort {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IDeveloperWorkflowEnvironmentPort for CountingEnvironmentPort {
        async fn environment_exists(
            &self,
            _scope: DeveloperWorkflowEnvironmentScope,
        ) -> Result<bool, RepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }
}
