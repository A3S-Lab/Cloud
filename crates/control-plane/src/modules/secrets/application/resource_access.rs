use crate::modules::secrets::domain::{ISecretRepository, Secret};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError, SecretId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// One Secrets visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// in Secrets and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SecretAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl SecretAccessScope {
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

/// Secrets-owned projection of an already-authorized request.
///
/// Identity remains the authorization authority. Entry adapters narrow that
/// decision into this immutable value, while Secrets owns resource-to-scope
/// resolution and conceals missing and denied records identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<SecretAccessScope>,
}

impl SecretAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = SecretAccessScope>) -> Self {
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

/// Resolves indirect Secret identifiers through the Secrets authority before authorization.
///
/// Secrets owns immutable project/environment identity and evaluates only its
/// local access value. This avoids both a second ownership registry and a
/// duplicated Identity policy engine.
#[derive(Clone)]
pub(crate) struct SecretResourceResolver {
    secrets: Arc<dyn ISecretRepository>,
}

impl SecretResourceResolver {
    pub fn new(secrets: Arc<dyn ISecretRepository>) -> Self {
        Self { secrets }
    }

    pub async fn secret(
        &self,
        organization_id: OrganizationId,
        secret_id: SecretId,
        access: &SecretAccess,
    ) -> ApplicationResult<Secret> {
        let secret = self
            .secrets
            .find(organization_id, secret_id)
            .await
            .map_err(map_secret_repository_error)?;
        if !access.environment_is_visible(secret.project_id, secret.environment_id) {
            return Err(secret_not_found());
        }
        Ok(secret)
    }
}

fn map_secret_repository_error(error: RepositoryError) -> ApplicationError {
    match error {
        RepositoryError::NotFound => secret_not_found(),
        error => error.into(),
    }
}

fn secret_not_found() -> ApplicationError {
    ApplicationError::NotFound("secret not found".into())
}

#[cfg(test)]
impl SecretAccess {
    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = SecretAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_visibility_is_exact_and_canonicalized() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let exact = SecretAccessScope::Environment {
            project_id,
            environment_id,
        };
        let access = SecretAccess::restricted([exact, exact]);

        assert_eq!(access.granted_scopes().collect::<Vec<_>>(), [exact]);
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), environment_id));
    }

    #[test]
    fn project_visibility_includes_descendant_environments() {
        let project_id = ProjectId::new();
        let access = SecretAccess::restricted([SecretAccessScope::Project { project_id }]);

        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
        assert!(SecretAccess::organization_wide()
            .environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }
}
