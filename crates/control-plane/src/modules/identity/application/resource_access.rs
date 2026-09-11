use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Identity visibility selector for Application CQRS that only needs environment scope.
///
/// Project grants cover every environment under that project. Environment grants
/// are exact. Node grants have no ownership meaning for inference-key visibility
/// and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentityAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl IdentityAccessScope {
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

/// Identity-owned projection of an already-authorized request for CQRS paths that
/// authorize by environment visibility (for example inference-key queries).
///
/// Presentation guards and grant management continue to use the domain evaluator;
/// Application queries consume this narrowed projection instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<IdentityAccessScope>,
}

impl IdentityAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = IdentityAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = IdentityAccessScope> + '_ {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_sees_every_environment() {
        let access = IdentityAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn project_grant_covers_environments_while_environment_grant_is_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project_access =
            IdentityAccess::restricted([IdentityAccessScope::Project { project_id }]);
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.environment_is_visible(ProjectId::new(), environment_id));

        let environment_access = IdentityAccess::restricted([IdentityAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
