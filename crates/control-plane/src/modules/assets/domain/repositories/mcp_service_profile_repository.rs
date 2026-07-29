use crate::modules::assets::domain::McpServiceProfile;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServiceProfileBinding {
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub profile: McpServiceProfile,
    pub created_at: DateTime<Utc>,
}

impl McpServiceProfileBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("MCP Service profile binding identity or timestamp is invalid".into());
        }
        McpServiceProfile::restore(self.profile.canonical_acl(), self.profile.digest().as_str())?;
        Ok(())
    }
}

#[async_trait]
pub trait IMcpServiceProfileRepository: Send + Sync {
    /// Bind an immutable canonical profile to one published MCP release.
    /// Repeating the identical binding is idempotent; different bytes or a
    /// different digest for the same release are a conflict.
    async fn bind_mcp_service_profile(
        &self,
        binding: McpServiceProfileBinding,
    ) -> Result<McpServiceProfileBinding, RepositoryError>;

    async fn find_mcp_service_profile(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<McpServiceProfileBinding>, RepositoryError>;
}
