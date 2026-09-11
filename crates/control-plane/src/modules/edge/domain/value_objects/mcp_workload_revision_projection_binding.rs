use crate::modules::edge::domain::{EdgeMcpServiceProfileProjectionBinding, McpRoutePolicySpec};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use chrono::{DateTime, Utc};

/// Edge-owned Workload revision fact for MCP Gateway projection.
///
/// Carries only the identity, release binding, and template slice planners and
/// compilers need. Full Workloads aggregates stay behind Infrastructure ACAs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeMcpWorkloadRevisionProjectionBinding {
    revision_id: WorkloadRevisionId,
    workload_id: WorkloadId,
    generation: u64,
    created_at: DateTime<Utc>,
    organization_id: OrganizationId,
    asset_id: AssetId,
    asset_release_id: AssetReleaseId,
    profile_digest: Sha256Digest,
    runtime_port: String,
    health_port_name: String,
    health_path: String,
    workload_aggregate_version: u64,
    workload_updated_at: DateTime<Utc>,
}

impl EdgeMcpWorkloadRevisionProjectionBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision_id: WorkloadRevisionId,
        workload_id: WorkloadId,
        generation: u64,
        created_at: DateTime<Utc>,
        organization_id: OrganizationId,
        asset_id: AssetId,
        asset_release_id: AssetReleaseId,
        profile_digest: Sha256Digest,
        runtime_port: impl Into<String>,
        health_port_name: impl Into<String>,
        health_path: impl Into<String>,
        workload_aggregate_version: u64,
        workload_updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let binding = Self {
            revision_id,
            workload_id,
            generation,
            created_at: canonical_timestamp(created_at),
            organization_id,
            asset_id,
            asset_release_id,
            profile_digest,
            runtime_port: runtime_port.into(),
            health_port_name: health_port_name.into(),
            health_path: health_path.into(),
            workload_aggregate_version,
            workload_updated_at: canonical_timestamp(workload_updated_at),
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.revision_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.generation == 0
            || self.workload_aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.workload_updated_at != canonical_timestamp(self.workload_updated_at)
        {
            return Err(
                "MCP Workload revision projection binding identity or freshness is invalid".into(),
            );
        }
        validate_port_name("runtime", &self.runtime_port)?;
        validate_port_name("health", &self.health_port_name)?;
        validate_literal_path(&self.health_path)?;
        if self.health_port_name != self.runtime_port {
            return Err(
                "MCP Workload revision projection health port must match its runtime port".into(),
            );
        }
        Ok(())
    }

    pub fn matches_profile(
        &self,
        profile: &EdgeMcpServiceProfileProjectionBinding,
    ) -> Result<(), String> {
        profile.validate()?;
        self.validate()?;
        if self.organization_id != profile.organization_id()
            || self.asset_id != profile.asset_id()
            || self.asset_release_id != profile.asset_release_id()
            || &self.profile_digest != profile.digest()
            || self.runtime_port != profile.runtime_port()
            || self.health_path != profile.health_path()
        {
            return Err(
                "MCP Workload revision projection differs from its bound Service profile".into(),
            );
        }
        Ok(())
    }

    pub fn matches_policy_spec(&self, spec: &McpRoutePolicySpec) -> Result<(), String> {
        self.validate()?;
        if self.organization_id != spec.organization_id
            || self.workload_id != spec.workload_id
            || self.asset_id != spec.asset_id
            || self.asset_release_id != spec.asset_release_id
            || self.profile_digest != spec.profile_digest
        {
            return Err(
                "MCP Workload revision projection differs from its route policy release binding"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn runtime_unit_id(&self) -> String {
        format!(
            "workload:{}:revision:{}",
            self.workload_id, self.revision_id
        )
    }

    pub const fn revision_id(&self) -> WorkloadRevisionId {
        self.revision_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
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

    pub const fn profile_digest(&self) -> &Sha256Digest {
        &self.profile_digest
    }

    pub fn runtime_port(&self) -> &str {
        &self.runtime_port
    }

    pub fn health_path(&self) -> &str {
        &self.health_path
    }

    pub const fn workload_aggregate_version(&self) -> u64 {
        self.workload_aggregate_version
    }

    pub const fn workload_updated_at(&self) -> DateTime<Utc> {
        self.workload_updated_at
    }
}

fn validate_port_name(kind: &str, name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || name
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(format!(
            "MCP Workload revision projection {kind} port is invalid"
        ));
    }
    Ok(())
}

fn validate_literal_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.contains("//")
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err("MCP Workload revision projection health path is invalid".into());
    }
    Ok(())
}
