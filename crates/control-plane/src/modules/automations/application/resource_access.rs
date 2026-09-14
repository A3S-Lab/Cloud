use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Automations visibility selector projected from an Identity decision.
///
/// Project grants cover every environment under that project. Environment grants
/// are exact. Node grants have no ownership meaning here and are discarded by
/// the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AutomationAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl AutomationAccessScope {
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

/// Automations-owned projection of an already-authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<AutomationAccessScope>,
}

impl AutomationAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = AutomationAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

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
    access: &AutomationAccess,
) -> ApplicationResult<()> {
    if access.environment_is_visible(project_id, environment_id) {
        return Ok(());
    }
    Err(environment_not_found())
}

pub(super) fn environment_not_found() -> ApplicationError {
    ApplicationError::NotFound("Automation environment not found".into())
}

pub(super) fn endpoint_not_found() -> ApplicationError {
    ApplicationError::NotFound("Automation webhook endpoint not found".into())
}

pub(super) fn definition_not_found() -> ApplicationError {
    ApplicationError::NotFound("Automation definition not found".into())
}

pub(super) fn revision_not_found() -> ApplicationError {
    ApplicationError::NotFound("Automation revision not found".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_grant_covers_descendant_environments() {
        let project_id = ProjectId::new();
        let access = AutomationAccess::restricted([AutomationAccessScope::Project { project_id }]);
        assert!(access.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn environment_grant_is_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let access = AutomationAccess::restricted([AutomationAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(access.environment_is_visible(project_id, environment_id));
        assert!(!access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
