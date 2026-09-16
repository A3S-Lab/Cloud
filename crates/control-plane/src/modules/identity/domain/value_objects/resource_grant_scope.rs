use crate::modules::shared_kernel::domain::{ApplicationId, EnvironmentId, NodeId, ProjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceGrantScope {
    Project {
        project_id: ProjectId,
    },
    Environment {
        project_id: ProjectId,
        environment_id: EnvironmentId,
    },
    Application {
        project_id: ProjectId,
        application_id: ApplicationId,
    },
    Node {
        node_id: NodeId,
    },
}

impl ResourceGrantScope {
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Environment { .. } => "environment",
            Self::Application { .. } => "application",
            Self::Node { .. } => "node",
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::Project { project_id }
            | Self::Environment { project_id, .. }
            | Self::Application { project_id, .. } => Some(project_id),
            Self::Node { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
        match self {
            Self::Environment { environment_id, .. } => Some(environment_id),
            Self::Project { .. } | Self::Application { .. } | Self::Node { .. } => None,
        }
    }

    pub const fn application_id(self) -> Option<ApplicationId> {
        match self {
            Self::Application { application_id, .. } => Some(application_id),
            Self::Project { .. } | Self::Environment { .. } | Self::Node { .. } => None,
        }
    }

    pub const fn node_id(self) -> Option<NodeId> {
        match self {
            Self::Node { node_id } => Some(node_id),
            Self::Project { .. } | Self::Environment { .. } | Self::Application { .. } => None,
        }
    }

    /// Grant coverage for Identity resource authorization.
    ///
    /// Project grants cover Environment descendants for restricted management
    /// navigation. Application grants are exact and never implied by Project or
    /// Environment grants — published application delivery requires an exact
    /// Application Resource Grant (APP0.3 / Rule 8).
    pub fn allows(self, resource: Self) -> bool {
        match (self, resource) {
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
            (
                Self::Application {
                    project_id: granted_project,
                    application_id: granted,
                },
                Self::Application {
                    project_id: requested_project,
                    application_id: requested,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_grants_cover_environments_without_covering_applications_or_nodes() {
        let project_id = ProjectId::new();
        let other_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let application_id = ApplicationId::new();
        let node_id = NodeId::new();
        let project = ResourceGrantScope::Project { project_id };

        assert!(project.allows(project));
        assert!(project.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }));
        assert!(!project.allows(ResourceGrantScope::Application {
            project_id,
            application_id,
        }));
        assert!(!project.allows(ResourceGrantScope::Environment {
            project_id: other_project_id,
            environment_id,
        }));
        assert!(!project.allows(ResourceGrantScope::Node { node_id }));
    }

    #[test]
    fn environment_application_and_node_grants_are_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let application_id = ApplicationId::new();
        let environment = ResourceGrantScope::Environment {
            project_id,
            environment_id,
        };
        let application = ResourceGrantScope::Application {
            project_id,
            application_id,
        };

        assert!(environment.allows(environment));
        assert!(!environment.allows(ResourceGrantScope::Project { project_id }));
        assert!(!environment.allows(ResourceGrantScope::Application {
            project_id,
            application_id,
        }));
        assert!(!environment.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id: EnvironmentId::new(),
        }));

        assert!(application.allows(application));
        assert!(!application.allows(ResourceGrantScope::Project { project_id }));
        assert!(!application.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }));
        assert!(!application.allows(ResourceGrantScope::Application {
            project_id,
            application_id: ApplicationId::new(),
        }));

        let node = ResourceGrantScope::Node {
            node_id: NodeId::new(),
        };
        assert!(node.allows(node));
        assert!(!node.allows(ResourceGrantScope::Project { project_id }));
    }

    #[test]
    fn application_scope_exposes_parent_project_without_environment_or_node() {
        let project_id = ProjectId::new();
        let application_id = ApplicationId::new();
        let scope = ResourceGrantScope::Application {
            project_id,
            application_id,
        };
        assert_eq!(scope.kind(), "application");
        assert_eq!(scope.project_id(), Some(project_id));
        assert_eq!(scope.application_id(), Some(application_id));
        assert_eq!(scope.environment_id(), None);
        assert_eq!(scope.node_id(), None);
    }
}
