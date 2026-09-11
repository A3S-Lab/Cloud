use crate::modules::shared_kernel::domain::NodeId;
use std::collections::BTreeSet;

/// One Fleet visibility selector projected from an Identity decision.
///
/// Fleet Application only needs node visibility and organization-wide policy
/// gates for node pools. Project and environment grants have no ownership
/// meaning here and are discarded by the root anti-corruption layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FleetAccessScope {
    Node { node_id: NodeId },
}

/// Fleet-owned projection of an already-authorized request.
///
/// Identity remains the authentication and authorization authority. Entry
/// adapters narrow that decision into this immutable value so Fleet Application
/// never imports Identity grant vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetAccess {
    organization_wide: bool,
    granted_scopes: BTreeSet<FleetAccessScope>,
}

impl FleetAccess {
    pub(crate) fn organization_wide() -> Self {
        Self {
            organization_wide: true,
            granted_scopes: BTreeSet::new(),
        }
    }

    pub(crate) fn restricted(granted_scopes: impl IntoIterator<Item = FleetAccessScope>) -> Self {
        Self {
            organization_wide: false,
            granted_scopes: granted_scopes.into_iter().collect(),
        }
    }

    pub(crate) const fn is_organization_wide(&self) -> bool {
        self.organization_wide
    }

    pub(crate) fn granted_scopes(&self) -> impl Iterator<Item = FleetAccessScope> + '_ {
        self.granted_scopes.iter().copied()
    }

    pub(crate) fn node_is_visible(&self, node_id: NodeId) -> bool {
        self.organization_wide
            || self
                .granted_scopes
                .contains(&FleetAccessScope::Node { node_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_wide_sees_every_node_and_authorizes_pool_policy() {
        let access = FleetAccess::organization_wide();
        assert!(access.is_organization_wide());
        assert!(access.node_is_visible(NodeId::new()));
    }

    #[test]
    fn restricted_access_exposes_only_exact_node_grants() {
        let node_id = NodeId::new();
        let access = FleetAccess::restricted([FleetAccessScope::Node { node_id }]);
        assert!(!access.is_organization_wide());
        assert!(access.node_is_visible(node_id));
        assert!(!access.node_is_visible(NodeId::new()));
    }
}
