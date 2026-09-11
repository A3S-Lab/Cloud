use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AssetId, AssetReleaseId, OrganizationId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const ACTIVE_MCP_WORKLOAD_REVISION_PROJECTION_SCHEMA: &str =
    "a3s.cloud.active-mcp-workload-revision-projection.v1";

/// Workloads-owned immutable proof that one running Workload's active revision
/// matches one exact MCP Service profile binding for Gateway projection.
///
/// Full Workload/revision aggregates and template interpretation remain inside
/// Workloads. Consumers receive only the minimum owner evidence needed to build
/// their own projection binding facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActiveMcpWorkloadRevisionProjection {
    schema: String,
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

pub(in crate::modules::workloads) struct ValidatedActiveMcpWorkloadRevisionProjection {
    pub revision_id: WorkloadRevisionId,
    pub workload_id: WorkloadId,
    pub generation: u64,
    pub created_at: DateTime<Utc>,
    pub organization_id: OrganizationId,
    pub asset_id: AssetId,
    pub asset_release_id: AssetReleaseId,
    pub profile_digest: Sha256Digest,
    pub runtime_port: String,
    pub health_port_name: String,
    pub health_path: String,
    pub workload_aggregate_version: u64,
    pub workload_updated_at: DateTime<Utc>,
}

impl ActiveMcpWorkloadRevisionProjection {
    pub(in crate::modules::workloads) fn from_validated(
        projection: ValidatedActiveMcpWorkloadRevisionProjection,
    ) -> Result<Self, String> {
        let value = Self {
            schema: ACTIVE_MCP_WORKLOAD_REVISION_PROJECTION_SCHEMA.into(),
            revision_id: projection.revision_id,
            workload_id: projection.workload_id,
            generation: projection.generation,
            created_at: canonical_timestamp(projection.created_at),
            organization_id: projection.organization_id,
            asset_id: projection.asset_id,
            asset_release_id: projection.asset_release_id,
            profile_digest: projection.profile_digest,
            runtime_port: projection.runtime_port,
            health_port_name: projection.health_port_name,
            health_path: projection.health_path,
            workload_aggregate_version: projection.workload_aggregate_version,
            workload_updated_at: canonical_timestamp(projection.workload_updated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ACTIVE_MCP_WORKLOAD_REVISION_PROJECTION_SCHEMA
            || self.revision_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.asset_id.as_uuid().is_nil()
            || self.asset_release_id.as_uuid().is_nil()
            || self.generation == 0
            || self.workload_aggregate_version == 0
            || self.created_at != canonical_timestamp(self.created_at)
            || self.workload_updated_at != canonical_timestamp(self.workload_updated_at)
            || self.runtime_port.is_empty()
            || self.health_port_name.is_empty()
            || self.health_path.is_empty()
            || self.health_port_name != self.runtime_port
        {
            return Err("active MCP Workload revision projection is invalid".into());
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
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

    pub fn profile_digest(&self) -> &Sha256Digest {
        &self.profile_digest
    }

    pub fn runtime_port(&self) -> &str {
        &self.runtime_port
    }

    pub fn health_port_name(&self) -> &str {
        &self.health_port_name
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_fact_rejects_identity_or_port_drift() {
        assert!(ActiveMcpWorkloadRevisionProjection::from_validated(
            ValidatedActiveMcpWorkloadRevisionProjection {
                revision_id: WorkloadRevisionId::new(),
                workload_id: WorkloadId::new(),
                generation: 1,
                created_at: Utc::now(),
                organization_id: OrganizationId::new(),
                asset_id: AssetId::new(),
                asset_release_id: AssetReleaseId::new(),
                profile_digest: Sha256Digest::parse(&format!("sha256:{}", "a".repeat(64)))
                    .expect("digest"),
                runtime_port: "mcp".into(),
                health_port_name: "other".into(),
                health_path: "/health".into(),
                workload_aggregate_version: 2,
                workload_updated_at: Utc::now(),
            },
        )
        .is_err());

        let fact = ActiveMcpWorkloadRevisionProjection::from_validated(
            ValidatedActiveMcpWorkloadRevisionProjection {
                revision_id: WorkloadRevisionId::new(),
                workload_id: WorkloadId::new(),
                generation: 1,
                created_at: Utc::now(),
                organization_id: OrganizationId::new(),
                asset_id: AssetId::new(),
                asset_release_id: AssetReleaseId::new(),
                profile_digest: Sha256Digest::parse(&format!("sha256:{}", "a".repeat(64)))
                    .expect("digest"),
                runtime_port: "mcp".into(),
                health_port_name: "mcp".into(),
                health_path: "/health".into(),
                workload_aggregate_version: 2,
                workload_updated_at: Utc::now(),
            },
        )
        .expect("valid projection");
        assert_eq!(fact.schema(), ACTIVE_MCP_WORKLOAD_REVISION_PROJECTION_SCHEMA);
    }
}
