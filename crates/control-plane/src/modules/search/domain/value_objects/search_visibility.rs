use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
use std::collections::BTreeSet;

/// A closed Search projection selector, not an authorization grant.
///
/// The root Presentation anti-corruption layer derives these selectors from
/// an Identity decision. Search may narrow result rows with them but never
/// issues, widens, or persists authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchVisibilityScope {
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

impl SearchVisibilityScope {
    fn allows(self, requested: Self) -> bool {
        match (self, requested) {
            (
                Self::Project {
                    project_id: granted,
                },
                Self::Project {
                    project_id: requested,
                }
                | Self::Environment {
                    project_id: requested,
                    ..
                },
            ) => granted.as_uuid() == requested.as_uuid(),
            (
                Self::Environment {
                    project_id: granted_project,
                    environment_id: granted,
                },
                Self::Environment {
                    project_id: requested_project,
                    environment_id: requested,
                },
            ) => {
                granted_project.as_uuid() == requested_project.as_uuid()
                    && granted.as_uuid() == requested.as_uuid()
            }
            (Self::Node { node_id: granted }, Self::Node { node_id: requested }) => {
                granted.as_uuid() == requested.as_uuid()
            }
            _ => false,
        }
    }
}

/// The immutable visibility projection attached to one Search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchVisibility {
    organization_wide: bool,
    granted_scopes: BTreeSet<SearchVisibilityScope>,
}

impl SearchVisibility {
    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = SearchVisibilityScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = SearchVisibilityScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    pub fn projected_resource_is_visible(
        &self,
        project_id: Option<ProjectId>,
        environment_id: Option<EnvironmentId>,
        node_id: Option<NodeId>,
    ) -> bool {
        if self.organization_wide {
            return true;
        }
        let requested = match (project_id, environment_id, node_id) {
            (Some(project_id), Some(environment_id), None) => SearchVisibilityScope::Environment {
                project_id,
                environment_id,
            },
            (Some(project_id), None, None) => SearchVisibilityScope::Project { project_id },
            (None, None, Some(node_id)) => SearchVisibilityScope::Node { node_id },
            _ => return false,
        };
        self.granted_scopes
            .iter()
            .any(|granted| granted.allows(requested))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_environment_and_node_visibility_is_closed_and_fail_closed() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let visibility = SearchVisibility::restricted([
            SearchVisibilityScope::Project { project_id },
            SearchVisibilityScope::Node { node_id },
        ]);

        assert!(visibility.projected_resource_is_visible(Some(project_id), None, None));
        assert!(visibility.projected_resource_is_visible(
            Some(project_id),
            Some(environment_id),
            None
        ));
        assert!(visibility.projected_resource_is_visible(None, None, Some(node_id)));
        assert!(!visibility.projected_resource_is_visible(
            Some(ProjectId::new()),
            Some(environment_id),
            None
        ));
        assert!(!visibility.projected_resource_is_visible(None, None, None));
        assert!(!visibility.projected_resource_is_visible(
            Some(project_id),
            Some(environment_id),
            Some(node_id)
        ));
    }

    #[test]
    fn environment_grants_do_not_expand_to_the_parent_project() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let visibility = SearchVisibility::restricted([SearchVisibilityScope::Environment {
            project_id,
            environment_id,
        }]);

        assert!(visibility.projected_resource_is_visible(
            Some(project_id),
            Some(environment_id),
            None
        ));
        assert!(!visibility.projected_resource_is_visible(Some(project_id), None, None));
        assert!(!visibility.projected_resource_is_visible(
            Some(project_id),
            Some(EnvironmentId::new()),
            None
        ));
    }

    #[test]
    fn duplicate_scopes_are_canonicalized_and_organization_access_is_explicit() {
        let project = SearchVisibilityScope::Project {
            project_id: ProjectId::new(),
        };
        let restricted = SearchVisibility::restricted([project, project]);
        assert_eq!(restricted.granted_scopes().collect::<Vec<_>>(), [project]);
        assert!(!restricted.is_organization_wide());
        assert!(!restricted.projected_resource_is_visible(None, None, None));

        let organization_wide = SearchVisibility::organization_wide();
        assert!(organization_wide.is_organization_wide());
        assert!(organization_wide.granted_scopes().next().is_none());
        assert!(organization_wide.projected_resource_is_visible(None, None, None));
    }
}
