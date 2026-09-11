use crate::modules::notifications::domain::NotificationScope;
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
use std::collections::BTreeSet;

/// One Notifications visibility selector projected from an Identity decision.
///
/// Project grants cover descendant environments. Environment and node grants are
/// exact. Organization-scoped notifications are visible to every authorized
/// principal. Identity remains the authority; this value only carries the
/// narrowed vocabulary Notifications needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationAccessScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
    Node {
        node_id: NodeId,
    },
}

impl NotificationAccessScope {
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
            Self::Node { .. } => false,
        }
    }

    fn allows_node(self, node_id: NodeId) -> bool {
        matches!(self, Self::Node { node_id: granted } if granted == node_id)
    }
}

/// Notifications-owned projection of an already-authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<NotificationAccessScope>,
}

impl NotificationAccess {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = NotificationAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = NotificationAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    pub fn project_is_authorized(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.authorizes_project(project_id))
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

    pub fn node_is_visible(&self, node_id: NodeId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.allows_node(node_id))
    }

    /// Whether a stored notification or alert-policy target scope is visible.
    pub fn scope_is_visible(&self, scope: NotificationScope) -> bool {
        match scope {
            NotificationScope::Organization => true,
            NotificationScope::Project { project_id } => self.project_is_authorized(project_id),
            NotificationScope::Environment {
                project_id,
                environment_id,
            } => self.environment_is_visible(project_id, environment_id),
            NotificationScope::Node { node_id } => self.node_is_visible(node_id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_sees_every_scoped_notification() {
        let access = NotificationAccess::organization_wide();
        assert!(access.scope_is_visible(NotificationScope::Organization));
        assert!(access.scope_is_visible(NotificationScope::Project {
            project_id: ProjectId::new()
        }));
        assert!(access.scope_is_visible(NotificationScope::Environment {
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
        }));
        assert!(access.scope_is_visible(NotificationScope::Node {
            node_id: NodeId::new()
        }));
    }

    #[test]
    fn restricted_access_preserves_project_environment_and_node_semantics() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let access = NotificationAccess::restricted([
            NotificationAccessScope::Environment {
                project_id,
                environment_id,
            },
            NotificationAccessScope::Node { node_id },
        ]);

        assert!(access.scope_is_visible(NotificationScope::Organization));
        assert!(access.scope_is_visible(NotificationScope::Environment {
            project_id,
            environment_id
        }));
        assert!(!access.scope_is_visible(NotificationScope::Project { project_id }));
        assert!(access.scope_is_visible(NotificationScope::Node { node_id }));
        assert!(!access.scope_is_visible(NotificationScope::Node {
            node_id: NodeId::new()
        }));

        let project_access =
            NotificationAccess::restricted([NotificationAccessScope::Project { project_id }]);
        assert!(project_access.project_is_authorized(project_id));
        assert!(project_access.environment_is_visible(project_id, environment_id));
        assert!(!project_access.node_is_visible(node_id));
    }
}
