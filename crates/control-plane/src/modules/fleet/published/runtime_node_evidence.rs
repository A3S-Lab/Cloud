use crate::modules::shared_kernel::domain::{
    canonical_timestamp, NodeId, NodePoolId, OrganizationId, Sha256Digest,
};
use a3s_runtime::contract::{RuntimeCapabilities, RuntimeObservation};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const RUNTIME_NODE_EVIDENCE_SCHEMA: &str = "a3s.cloud.runtime-node-evidence.v1";

/// Fleet-owned immutable snapshot of the current Node session, its current
/// capability document, and one exact accepted Runtime observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeNodeEvidence {
    schema: String,
    organization_id: OrganizationId,
    node_pool_id: NodePoolId,
    node_pool_aggregate_version: u64,
    node_pool_spec_digest: Sha256Digest,
    node_id: NodeId,
    node_aggregate_version: u64,
    agent_instance_id: Uuid,
    node_capabilities_digest: Sha256Digest,
    node_last_observed_at: DateTime<Utc>,
    runtime_capabilities: RuntimeCapabilities,
    runtime_report_id: Uuid,
    runtime_observed_at: DateTime<Utc>,
    runtime_received_at: DateTime<Utc>,
    runtime_observation: RuntimeObservation,
}

pub(in crate::modules::fleet) struct ValidatedRuntimeNodeEvidenceProjection {
    pub organization_id: OrganizationId,
    pub node_pool_id: NodePoolId,
    pub node_pool_aggregate_version: u64,
    pub node_pool_spec_digest: String,
    pub node_id: NodeId,
    pub node_aggregate_version: u64,
    pub agent_instance_id: Uuid,
    pub node_capabilities_digest: String,
    pub node_last_observed_at: DateTime<Utc>,
    pub runtime_capabilities: RuntimeCapabilities,
    pub runtime_report_id: Uuid,
    pub runtime_observed_at: DateTime<Utc>,
    pub runtime_received_at: DateTime<Utc>,
    pub runtime_observation: RuntimeObservation,
}

impl RuntimeNodeEvidence {
    pub(in crate::modules::fleet) fn from_validated_node(
        projection: ValidatedRuntimeNodeEvidenceProjection,
    ) -> Result<Self, String> {
        let observed_at_ms = i64::try_from(projection.runtime_observation.observed_at_ms)
            .map_err(|_| "Runtime observation timestamp exceeds the supported range")?;
        let runtime_observed_at = DateTime::<Utc>::from_timestamp_millis(observed_at_ms)
            .ok_or("Runtime observation timestamp exceeds the supported range")?;
        if projection.runtime_observed_at.timestamp_millis() != observed_at_ms {
            return Err(
                "stored Runtime report time does not match its millisecond protocol time".into(),
            );
        }
        let value = Self {
            schema: RUNTIME_NODE_EVIDENCE_SCHEMA.into(),
            organization_id: projection.organization_id,
            node_pool_id: projection.node_pool_id,
            node_pool_aggregate_version: projection.node_pool_aggregate_version,
            node_pool_spec_digest: Sha256Digest::parse(projection.node_pool_spec_digest)?,
            node_id: projection.node_id,
            node_aggregate_version: projection.node_aggregate_version,
            agent_instance_id: projection.agent_instance_id,
            node_capabilities_digest: Sha256Digest::parse(projection.node_capabilities_digest)?,
            node_last_observed_at: canonical_timestamp(projection.node_last_observed_at),
            runtime_capabilities: projection.runtime_capabilities,
            runtime_report_id: projection.runtime_report_id,
            runtime_observed_at,
            runtime_received_at: canonical_timestamp(projection.runtime_received_at),
            runtime_observation: projection.runtime_observation,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.runtime_capabilities.validate()?;
        self.runtime_observation.validate()?;
        let capabilities_document = serde_json::to_value(&self.runtime_capabilities)
            .map_err(|error| format!("could not encode Runtime capabilities: {error}"))?;
        let capabilities_bytes = serde_json::to_vec(&capabilities_document)
            .map_err(|error| format!("could not encode Runtime capabilities: {error}"))?;
        let computed_capabilities_digest = Sha256Digest::from_bytes(&capabilities_bytes);
        let observed_at_ms = i64::try_from(self.runtime_observation.observed_at_ms)
            .map_err(|_| "Runtime observation timestamp exceeds the supported range")?;
        let contract_observed_at = DateTime::<Utc>::from_timestamp_millis(observed_at_ms)
            .ok_or("Runtime observation timestamp exceeds the supported range")?;
        if self.schema != RUNTIME_NODE_EVIDENCE_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.node_pool_id.as_uuid().is_nil()
            || self.node_pool_aggregate_version == 0
            || self.node_id.as_uuid().is_nil()
            || self.node_aggregate_version == 0
            || self.agent_instance_id.is_nil()
            || self.runtime_report_id.is_nil()
        {
            return Err("Runtime Node evidence identity or version is invalid".into());
        }
        if Sha256Digest::parse(self.node_pool_spec_digest.as_str())? != self.node_pool_spec_digest
            || Sha256Digest::parse(self.node_capabilities_digest.as_str())?
                != self.node_capabilities_digest
            || self.node_capabilities_digest != computed_capabilities_digest
        {
            return Err("Runtime Node evidence digest is invalid".into());
        }
        if self.node_last_observed_at != canonical_timestamp(self.node_last_observed_at)
            || self.runtime_observed_at != canonical_timestamp(self.runtime_observed_at)
            || self.runtime_received_at != canonical_timestamp(self.runtime_received_at)
        {
            return Err("Runtime Node evidence timestamp precision is invalid".into());
        }
        if self.runtime_observed_at != contract_observed_at {
            return Err("Runtime Node evidence protocol time is inconsistent".into());
        }
        if self.runtime_received_at < self.runtime_observed_at
            || self.node_last_observed_at < self.runtime_observed_at
        {
            return Err("Runtime Node evidence chronology is reordered".into());
        }
        if self.runtime_observation.provider_build.as_deref()
            != Some(self.runtime_capabilities.provider_build.as_str())
        {
            return Err("Runtime Node evidence provider build is invalid".into());
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn node_pool_id(&self) -> NodePoolId {
        self.node_pool_id
    }

    pub const fn node_pool_aggregate_version(&self) -> u64 {
        self.node_pool_aggregate_version
    }

    pub const fn node_pool_spec_digest(&self) -> &Sha256Digest {
        &self.node_pool_spec_digest
    }

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn node_aggregate_version(&self) -> u64 {
        self.node_aggregate_version
    }

    pub const fn agent_instance_id(&self) -> Uuid {
        self.agent_instance_id
    }

    pub const fn node_capabilities_digest(&self) -> &Sha256Digest {
        &self.node_capabilities_digest
    }

    pub const fn node_last_observed_at(&self) -> DateTime<Utc> {
        self.node_last_observed_at
    }

    pub const fn runtime_capabilities(&self) -> &RuntimeCapabilities {
        &self.runtime_capabilities
    }

    pub const fn runtime_report_id(&self) -> Uuid {
        self.runtime_report_id
    }

    pub const fn runtime_observed_at(&self) -> DateTime<Utc> {
        self.runtime_observed_at
    }

    pub const fn runtime_received_at(&self) -> DateTime<Utc> {
        self.runtime_received_at
    }

    pub const fn runtime_observation(&self) -> &RuntimeObservation {
        &self.runtime_observation
    }
}
