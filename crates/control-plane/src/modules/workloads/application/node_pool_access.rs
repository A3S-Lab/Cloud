use crate::modules::shared_kernel::domain::{NodePoolId, OrganizationId, RepositoryError};
use async_trait::async_trait;

/// Exact Fleet-owned node-pool identity required by Workloads pool-selection checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkloadsNodePoolScope {
    organization_id: OrganizationId,
    node_pool_id: NodePoolId,
}

impl WorkloadsNodePoolScope {
    pub fn new(organization_id: OrganizationId, node_pool_id: NodePoolId) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            node_pool_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.node_pool_id.as_uuid().is_nil() {
            return Err("Workloads node-pool scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn node_pool_id(self) -> NodePoolId {
        self.node_pool_id
    }
}

/// Workloads-owned read port for the Fleet node-pool authority.
#[async_trait]
pub trait IWorkloadsNodePoolAccess: Send + Sync {
    async fn node_pool_exists(
        &self,
        scope: WorkloadsNodePoolScope,
    ) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let node_pool_id = NodePoolId::new();
        assert!(WorkloadsNodePoolScope::new(organization_id, node_pool_id).is_ok());
        assert!(
            WorkloadsNodePoolScope::new(OrganizationId::from_uuid(Uuid::nil()), node_pool_id,)
                .is_err()
        );
        assert!(
            WorkloadsNodePoolScope::new(organization_id, NodePoolId::from_uuid(Uuid::nil()),)
                .is_err()
        );
    }
}
