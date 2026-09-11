use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId, Sha256Digest,
};
use a3s_cloud_contracts::{McpServiceProfileProjection, MCP_PROTOCOL_VERSION};
use chrono::{DateTime, Utc};

/// Edge-owned MCP Service profile binding fact for Gateway projection.
///
/// Carries only the identity and profile fields planners/compilers need.
/// Full Assets ACL reconstruction stays behind Infrastructure adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeMcpServiceProfileProjectionBinding {
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    digest: Sha256Digest,
    protocol_versions: Vec<String>,
    endpoint_path: String,
    runtime_port: String,
    health_path: String,
    request_sse: bool,
    subscriptions: bool,
    max_request_bytes: u64,
    max_response_bytes: u64,
    created_at: DateTime<Utc>,
}

impl EdgeMcpServiceProfileProjectionBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        digest: Sha256Digest,
        protocol_versions: Vec<String>,
        endpoint_path: impl Into<String>,
        runtime_port: impl Into<String>,
        health_path: impl Into<String>,
        request_sse: bool,
        subscriptions: bool,
        max_request_bytes: u64,
        max_response_bytes: u64,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            asset_id,
            asset_release_id,
            digest,
            protocol_versions,
            endpoint_path: endpoint_path.into(),
            runtime_port: runtime_port.into(),
            health_path: health_path.into(),
            request_sse,
            subscriptions,
            max_request_bytes,
            max_response_bytes,
            created_at: canonical_timestamp(created_at),
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err(
                "MCP Service profile projection binding identity or timestamp is invalid".into(),
            );
        }
        if self.protocol_versions != [MCP_PROTOCOL_VERSION] {
            return Err(format!(
                "MCP Service profile projection must support exactly {MCP_PROTOCOL_VERSION}"
            ));
        }
        validate_literal_path("endpoint", &self.endpoint_path)?;
        validate_literal_path("health", &self.health_path)?;
        if self.endpoint_path == self.health_path {
            return Err("MCP endpoint and health paths must be distinct".into());
        }
        if self.runtime_port.is_empty()
            || self.runtime_port.len() > 64
            || self
                .runtime_port
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err("MCP Service profile projection runtime port is invalid".into());
        }
        if self.subscriptions && !self.request_sse {
            return Err("MCP subscriptions require request-scoped SSE".into());
        }
        if self.max_request_bytes == 0 || self.max_response_bytes == 0 {
            return Err("MCP Service profile projection byte bounds must be positive".into());
        }
        Ok(())
    }

    pub fn gateway_projection(&self) -> McpServiceProfileProjection {
        McpServiceProfileProjection {
            profile_digest: self.digest.to_string(),
            protocol_versions: self.protocol_versions.clone(),
            path: self.endpoint_path.clone(),
            request_sse: self.request_sse,
            subscriptions: self.subscriptions,
            max_request_bytes: self.max_request_bytes,
            max_response_bytes: self.max_response_bytes,
        }
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn asset_id(&self) -> AssetId {
        self.asset_id
    }

    pub const fn asset_release_id(&self) -> AssetReleaseId {
        self.asset_release_id
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }

    pub fn runtime_port(&self) -> &str {
        &self.runtime_port
    }

    pub fn health_path(&self) -> &str {
        &self.health_path
    }

    pub fn endpoint_path(&self) -> &str {
        &self.endpoint_path
    }

    pub const fn max_request_bytes(&self) -> u64 {
        self.max_request_bytes
    }

    pub const fn max_response_bytes(&self) -> u64 {
        self.max_response_bytes
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

fn validate_literal_path(kind: &str, path: &str) -> Result<(), String> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.contains("//")
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(format!(
            "MCP Service profile projection {kind} path is invalid"
        ));
    }
    Ok(())
}
