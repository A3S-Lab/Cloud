use crate::modules::assets::domain::{McpServiceProfile, McpServiceProfileBound};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, IdempotencyRequest, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServiceProfileWriteReference {
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub profile_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServiceProfileWrite {
    pub binding: McpServiceProfileBinding,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct BindMcpServiceProfileWrite {
    pub binding: McpServiceProfileBinding,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl BindMcpServiceProfileWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.event.event_id.is_nil()
            || self.event.correlation_id.is_nil()
            || self
                .event
                .causation_id
                .is_some_and(|causation_id| causation_id.is_nil())
            || self.event.event_key != "asset.mcp-service-profile.bound"
            || self.event.schema_version != 1
            || self.event.organization_id() != Some(self.binding.organization_id.as_uuid())
            || self.event.aggregate_id != self.binding.asset_release_id.as_uuid()
            || self.event.aggregate_version != 1
            || self.event.occurred_at != self.binding.created_at
        {
            return Err("MCP Service profile binding and domain event are inconsistent".into());
        }
        let payload: McpServiceProfileBound = serde_json::from_value(self.event.payload.clone())
            .map_err(|error| {
                format!("MCP Service profile bound event payload is invalid: {error}")
            })?;
        if payload.organization_id != self.binding.organization_id.as_uuid()
            || payload.asset_id != self.binding.asset_id.as_uuid()
            || payload.asset_release_id != self.binding.asset_release_id.as_uuid()
            || payload.profile_digest != self.binding.profile.digest().as_str()
        {
            return Err("MCP Service profile bound event payload is inconsistent".into());
        }
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
        bundle: BindMcpServiceProfileWrite,
    ) -> Result<McpServiceProfileWrite, RepositoryError>;

    async fn find_mcp_service_profile(
        &self,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
    ) -> Result<Option<McpServiceProfileBinding>, RepositoryError>;
}
