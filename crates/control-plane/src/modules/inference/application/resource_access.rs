use crate::modules::shared_kernel::domain::{EnvironmentId, ProjectId};
use std::collections::BTreeSet;

/// One Inference visibility selector projected from an Identity decision.
///
/// Project grants cover every environment under that project. Environment grants
/// are exact. Node grants have no ownership meaning here and are discarded by
/// the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InferenceAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
}

impl InferenceAccessScope {
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

/// Inference-owned projection of an already-authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<InferenceAccessScope>,
}

impl InferenceAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = InferenceAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = InferenceAccessScope> + '_ {
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
        let access = InferenceAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.environment_is_visible(ProjectId::new(), EnvironmentId::new()));
    }

    #[test]
    fn project_grant_covers_environments_while_environment_grant_is_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let project_access =
            InferenceAccess::restricted([InferenceAccessScope::Project { project_id }]);
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.environment_is_visible(ProjectId::new(), environment_id));

        let environment_access = InferenceAccess::restricted([InferenceAccessScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(environment_access.environment_is_visible(project_id, environment_id));
        assert!(!environment_access.environment_is_visible(project_id, EnvironmentId::new()));
    }
}
