use crate::modules::shared_kernel::domain::{NodeId, OrganizationId, RepositoryError};
use async_trait::async_trait;

/// Exact Fleet-owned node identity required by Identity resource-grant commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityNodeScope {
    organization_id: OrganizationId,
    node_id: NodeId,
}

impl IdentityNodeScope {
    pub fn new(organization_id: OrganizationId, node_id: NodeId) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            node_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.node_id.as_uuid().is_nil() {
            return Err("Identity node scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn node_id(self) -> NodeId {
        self.node_id
    }
}

/// Identity-owned read port for the Fleet node authority.
#[async_trait]
pub trait IIdentityNodeAccess: Send + Sync {
    async fn node_exists(&self, scope: IdentityNodeScope) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let node_id = NodeId::new();
        assert!(IdentityNodeScope::new(organization_id, node_id).is_ok());
        assert!(IdentityNodeScope::new(OrganizationId::from_uuid(Uuid::nil()), node_id,).is_err());
    }
}
