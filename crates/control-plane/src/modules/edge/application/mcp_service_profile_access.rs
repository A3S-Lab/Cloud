use crate::modules::edge::domain::EdgeMcpServiceProfileAdmission;
use crate::modules::shared_kernel::domain::{
    AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;

/// Exact Assets-owned MCP Service profile identity required by Edge route-policy
/// admission. Application reads only the Edge-owned admission fact through this
/// port and must not reach Assets through its profile repository trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeMcpServiceProfileScope {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
}

impl EdgeMcpServiceProfileScope {
    pub fn new(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            asset_id,
            asset_release_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
        {
            return Err("Edge MCP Service profile scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(self) -> AssetReleaseId {
        self.asset_release_id
    }
}

/// Edge-owned read port for Assets MCP Service profile admission facts.
#[async_trait]
pub trait IEdgeMcpServiceProfileAccess: Send + Sync {
    async fn find_bound_profile(
        &self,
        scope: EdgeMcpServiceProfileScope,
    ) -> Result<Option<EdgeMcpServiceProfileAdmission>, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let asset_id = AssetId::new();
        let asset_release_id = AssetReleaseId::new();
        assert!(
            EdgeMcpServiceProfileScope::new(organization_id, asset_id, asset_release_id).is_ok()
        );
        assert!(EdgeMcpServiceProfileScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            asset_id,
            asset_release_id,
        )
        .is_err());
    }
}
