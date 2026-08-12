use crate::modules::identity::domain::value_objects::{MembershipRole, ResourceGrantScope};
use crate::modules::shared_kernel::domain::{EnvironmentId, NodeId, ProjectId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceAccessEvaluator {
    organization_wide: bool,
    granted_scopes: BTreeSet<ResourceGrantScope>,
}

impl ResourceAccessEvaluator {
    pub fn for_membership(
        role: MembershipRole,
        granted_scopes: impl IntoIterator<Item = ResourceGrantScope>,
    ) -> Self {
        if role == MembershipRole::Restricted {
            Self {
                organization_wide: false,
                granted_scopes: granted_scopes.into_iter().collect(),
            }
        } else {
            Self::organization_wide()
        }
    }

    pub fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub fn restricted(granted_scopes: impl IntoIterator<Item = ResourceGrantScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub fn has_any_visible_resource(&self) -> bool {
        self.organization_wide || !self.granted_scopes.is_empty()
    }

    pub fn allows(&self, resource: ResourceGrantScope) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|granted| granted.allows(resource))
    }

    pub fn project_is_visible_in_collection(&self, project_id: ProjectId) -> bool {
        self.organization_wide
            || self.granted_scopes.iter().any(|granted| {
                granted.project_id().is_some_and(|granted_project_id| {
                    granted_project_id.as_uuid() == project_id.as_uuid()
                })
            })
    }

    pub fn environment_is_visible(
        &self,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> bool {
        self.allows(ResourceGrantScope::Environment {
            project_id,
            environment_id,
        })
    }

    pub fn node_is_visible(&self, node_id: NodeId) -> bool {
        self.allows(ResourceGrantScope::Node { node_id })
    }

    pub fn projected_resource_is_visible(
        &self,
        project_id: Option<ProjectId>,
        environment_id: Option<EnvironmentId>,
        node_id: Option<NodeId>,
    ) -> bool {
        match (project_id, environment_id, node_id) {
            (_, _, _) if self.organization_wide => true,
            (Some(project_id), Some(environment_id), None) => {
                self.environment_is_visible(project_id, environment_id)
            }
            (Some(project_id), None, None) => {
                self.allows(ResourceGrantScope::Project { project_id })
            }
            (None, None, Some(node_id)) => self.node_is_visible(node_id),
            _ => false,
        }
    }

    pub fn has_project_visibility(&self) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.project_id().is_some())
    }

    pub fn has_project_authority(&self) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| matches!(scope, ResourceGrantScope::Project { .. }))
    }

    pub fn has_node_visibility(&self) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .iter()
                .any(|scope| scope.node_id().is_some())
    }

    pub fn granted_scopes(&self) -> impl Iterator<Item = ResourceGrantScope> + '_ {
        self.granted_scopes.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_restricted_roles_remain_organization_wide() {
        for role in [
            MembershipRole::Owner,
            MembershipRole::Admin,
            MembershipRole::Member,
        ] {
            let evaluator = ResourceAccessEvaluator::for_membership(role, []);
            assert!(evaluator.is_organization_wide());
            assert!(evaluator.allows(ResourceGrantScope::Project {
                project_id: ProjectId::new(),
            }));
        }
    }

    #[test]
    fn project_grants_include_descendant_environments() {
        let project_id = ProjectId::new();
        let evaluator = ResourceAccessEvaluator::for_membership(
            MembershipRole::Restricted,
            [ResourceGrantScope::Project { project_id }],
        );
        assert!(!evaluator.is_organization_wide());
        assert!(evaluator.project_is_visible_in_collection(project_id));
        assert!(evaluator.environment_is_visible(project_id, EnvironmentId::new()));
        assert!(!evaluator.node_is_visible(NodeId::new()));
    }

    #[test]
    fn environment_grants_expose_parent_only_for_collection_navigation() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let evaluator = ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(evaluator.project_is_visible_in_collection(project_id));
        assert!(!evaluator.allows(ResourceGrantScope::Project { project_id }));
        assert!(evaluator.environment_is_visible(project_id, environment_id));
        assert!(!evaluator.environment_is_visible(project_id, EnvironmentId::new()));
    }

    #[test]
    fn duplicate_scopes_are_canonicalized() {
        let node = ResourceGrantScope::Node {
            node_id: NodeId::new(),
        };
        let evaluator = ResourceAccessEvaluator::restricted([node, node]);
        assert_eq!(evaluator.granted_scopes().collect::<Vec<_>>(), vec![node]);
    }

    #[test]
    fn projected_resources_fail_closed_without_a_supported_scope() {
        let project_id = ProjectId::new();
        let environment_id = EnvironmentId::new();
        let node_id = NodeId::new();
        let evaluator = ResourceAccessEvaluator::restricted([ResourceGrantScope::Environment {
            project_id,
            environment_id,
        }]);
        assert!(evaluator.projected_resource_is_visible(
            Some(project_id),
            Some(environment_id),
            None
        ));
        assert!(!evaluator.projected_resource_is_visible(Some(project_id), None, None));
        assert!(!evaluator.projected_resource_is_visible(None, None, Some(node_id)));
        assert!(!evaluator.projected_resource_is_visible(None, None, None));
        assert!(!evaluator.projected_resource_is_visible(
            Some(project_id),
            Some(environment_id),
            Some(node_id)
        ));
    }
}
