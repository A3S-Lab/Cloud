use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Connectors visibility selector projected from an Identity decision.
///
/// Project grants cover every environment under that project. Environment grants
/// are exact. Node grants have no ownership meaning here and are discarded by
/// the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectorAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl ConnectorAccessScope {
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

/// Connectors-owned projection of an already-authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<ConnectorAccessScope>,
}

impl ConnectorAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = ConnectorAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = ConnectorAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    /// Project grants cover all descendant environments; environment grants are exact.
    pub fn environment_is_visible(
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

pub(super) fn environment(
    project_id: ProjectId,
    environment_id: EnvironmentId,
    access: &ConnectorAccess,
) -> ApplicationResult<()> {
    if access.environment_is_visible(project_id, environment_id) {
        return Ok(());
    }
    Err(environment_not_found())
}

pub(super) fn environment_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector environment not found".into())
}

pub(super) fn profile_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector profile not found".into())
}

pub(super) fn revision_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector revision not found".into())
}

pub(super) fn revision_revocation_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector revision revocation not found".into())
}

pub(super) fn evidence_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector execution evidence not found".into())
}

pub(super) fn attempt_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector execution attempt not found".into())
}

pub(super) fn attempt_resolution_not_found() -> ApplicationError {
    ApplicationError::NotFound("Connector execution attempt resolution not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_sees_every_environment() {
        let access = ConnectorAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn project_grant_covers_environments_while_environment_grant_is_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project_access =
            ConnectorAccess::restricted([ConnectorAccessScope::Project { project_id }]);
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.environment_is_visible(ProjectId::new(), environment_id));

        let environment_access = ConnectorAccess::restricted([ConnectorAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
