use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Plugins visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// for Plugins assignment inventory and are discarded by the root ACA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PluginAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl PluginAccessScope {
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

/// Plugins-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<PluginAccessScope>,
}

impl PluginAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = PluginAccessScope>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_surfaces_require_matching_authority() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        assert!(
            PluginAccess::organization_wide().environment_is_visible(project_id, environment_id)
        );
        assert!(
            PluginAccess::restricted([PluginAccessScope::Project { project_id }])
                .environment_is_visible(project_id, environment_id)
        );
        assert!(!PluginAccess::restricted([]).environment_is_visible(project_id, environment_id));
    }
}
