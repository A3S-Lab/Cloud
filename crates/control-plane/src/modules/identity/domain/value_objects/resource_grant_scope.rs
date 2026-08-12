use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
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
    Node {
        node_id: NodeId,
    },
}

impl ResourceGrantScope {
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Environment { .. } => "environment",
            Self::Node { .. } => "node",
        }
    }

    pub const fn project_id(self) -> Option<ProjectId> {
        match self {
            Self::Project { project_id } | Self::Environment { project_id, .. } => Some(project_id),
            Self::Node { .. } => None,
        }
    }

    pub const fn environment_id(self) -> Option<EnvironmentId> {
        match self {
            Self::Environment { environment_id, .. } => Some(environment_id),
            Self::Project { .. } | Self::Node { .. } => None,
        }
    }

    pub const fn node_id(self) -> Option<NodeId> {
        match self {
            Self::Node { node_id } => Some(node_id),
            Self::Project { .. } | Self::Environment { .. } => None,
        }
    }

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
    fn project_grants_cover_environment_descendants_without_crossing_scope_kinds() {
        let project_id = ProjectId::new();
        let other_project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let project = ResourceGrantScope::Project { project_id };

        assert!(project.allows(project));
        assert!(project.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }));
        assert!(!project.allows(ResourceGrantScope::Environment {
            project_id: other_project_id,
            environment_id,
        }));
        assert!(!project.allows(ResourceGrantScope::Node { node_id }));
    }

    #[test]
    fn environment_and_node_grants_are_exact() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let environment = ResourceGrantScope::Environment {
            project_id,
            environment_id,
        };
        assert!(environment.allows(environment));
        assert!(!environment.allows(ResourceGrantScope::Project { project_id }));
        assert!(!environment.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id: EnvironmentId::new(),
        }));

        let node = ResourceGrantScope::Node {
            node_id: NodeId::new(),
        };
        assert!(node.allows(node));
        assert!(!node.allows(ResourceGrantScope::Node {
            node_id: NodeId::new(),
        }));
    }
}
