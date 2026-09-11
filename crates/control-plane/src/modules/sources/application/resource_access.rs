use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Sources visibility selector projected from an Identity decision.
///
/// Project selectors include descendant environments; environment selectors
/// expose only one exact environment. Node selectors have no ownership meaning
/// for Sources and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SourceAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl SourceAccessScope {
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

/// Sources-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value. Environment
/// existence continues to live behind `ISourceEnvironmentAccess`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<SourceAccessScope>,
}

impl SourceAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = SourceAccessScope>) -> Self {
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
            SourceAccess::organization_wide().environment_is_visible(project_id, environment_id)
        );
        assert!(
            SourceAccess::restricted([SourceAccessScope::Project { project_id }])
                .environment_is_visible(project_id, environment_id)
        );
        assert!(
            SourceAccess::restricted([SourceAccessScope::Environment {
                project_id,
                environment_id,
            }])
            .environment_is_visible(project_id, environment_id)
        );
        assert!(
            !SourceAccess::restricted([SourceAccessScope::Environment {
                project_id,
                environment_id: EnvironmentId::new(),
            }])
            .environment_is_visible(project_id, environment_id)
        );
        assert!(!SourceAccess::restricted([]).environment_is_visible(project_id, environment_id));
    }
}
