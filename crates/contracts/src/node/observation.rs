use a3s_runtime::contract::{
    RuntimeCapabilities, RuntimeLogChunk, RuntimeLogDiscontinuityReason, RuntimeObservation,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{validate_sha256, validate_single_line, validate_uuid, GatewaySnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeHeartbeat {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub observed_at: DateTime<Utc>,
    pub agent_version: String,
    pub runtime_capabilities: RuntimeCapabilities,
}

impl NodeHeartbeat {
    pub const SCHEMA: &'static str = "a3s.cloud.node-heartbeat.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node heartbeat schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        validate_single_line("agent version", &self.agent_version, 255)?;
        self.runtime_capabilities.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeObservationReport {
    pub report_id: Uuid,
    pub command_id: Option<Uuid>,
    pub observed_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

impl RuntimeObservationReport {
    fn validate(&self) -> Result<(), String> {
        validate_uuid("report_id", self.report_id)?;
        if let Some(command_id) = self.command_id {
            validate_uuid("command_id", command_id)?;
        }
        self.observation.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeObservationBatch {
    pub schema: String,
    pub node_id: Uuid,
    pub agent_instance_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub heartbeat: NodeHeartbeat,
    pub observations: Vec<RuntimeObservationReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeObservationReceipt {
    pub schema: String,
    pub node_id: Uuid,
    pub heartbeat_observed_at: DateTime<Utc>,
    pub accepted_reports: u16,
    pub replayed_reports: u16,
}

impl NodeObservationReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-observation-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node observation receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        if usize::from(self.accepted_reports) + usize::from(self.replayed_reports) > 256 {
            return Err("node observation receipt exceeds the batch bound".into());
        }
        Ok(())
    }
}

impl NodeObservationBatch {
    pub const SCHEMA: &'static str = "a3s.cloud.node-observation-batch.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node observation batch schema {:?}",
                self.schema
            ));
        }
        validate_uuid("node_id", self.node_id)?;
        validate_uuid("agent_instance_id", self.agent_instance_id)?;
        self.heartbeat.validate()?;
        if self.heartbeat.node_id != self.node_id
            || self.heartbeat.agent_instance_id != self.agent_instance_id
        {
            return Err("node observation batch identity does not match its heartbeat".into());
        }
        if self.observations.len() > 256 {
            return Err("node observation batch exceeds 256 entries".into());
        }
        for observation in &self.observations {
            observation.validate()?;
            if observation.observed_at > self.sent_at {
                return Err("Runtime observation is newer than its enclosing batch".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLogChunkReport {
    pub unit_id: String,
    pub generation: u64,
    pub chunk: RuntimeLogChunk,
    pub checksum: String,
}

impl NodeLogChunkReport {
    pub fn validate(&self) -> Result<(), String> {
        validate_single_line("Runtime unit ID", &self.unit_id, 512)?;
        if self.generation == 0 {
            return Err("log chunk generation must be positive".into());
        }
        self.chunk.validate()?;
        validate_sha256("log chunk checksum", &self.checksum)?;
        let expected = format!("sha256:{:x}", Sha256::digest(self.chunk.data.as_bytes()));
        if self.checksum != expected {
            return Err("log chunk checksum does not match its data".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLogGapReport {
    pub unit_id: String,
    pub generation: u64,
    pub cursor: Option<String>,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub reason: RuntimeLogDiscontinuityReason,
}

impl NodeLogGapReport {
    pub fn validate(&self) -> Result<(), String> {
        validate_single_line("Runtime unit ID", &self.unit_id, 512)?;
        if self.generation == 0 {
            return Err("log gap generation must be positive".into());
        }
        if self
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 1024 || cursor.contains('\0'))
        {
            return Err("log gap cursor is invalid".into());
        }
        if self.reason == RuntimeLogDiscontinuityReason::CursorLost && self.cursor.is_none() {
            return Err("cursor-loss log gap must bind the lost cursor".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLogChunkBatch {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub sent_at: DateTime<Utc>,
    pub chunks: Vec<NodeLogChunkReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<NodeLogGapReport>,
}

impl NodeLogChunkBatch {
    pub const SCHEMA: &'static str = "a3s.cloud.node-log-chunk-batch.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node log batch schema {:?}",
                self.schema
            ));
        }
        validate_uuid("batch_id", self.batch_id)?;
        validate_uuid("node_id", self.node_id)?;
        let record_count = self
            .chunks
            .len()
            .checked_add(self.gaps.len())
            .ok_or_else(|| "node log batch record count overflowed".to_string())?;
        if record_count == 0 || record_count > 256 {
            return Err("node log batch must contain 1 to 256 records".into());
        }
        let mut total_data_bytes = 0_usize;
        let mut identities = std::collections::BTreeSet::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut last_chunk_sequences = std::collections::BTreeMap::new();
        let mut chunk_targets = std::collections::BTreeSet::new();
        for chunk in &self.chunks {
            chunk.validate()?;
            let target = (chunk.unit_id.as_str(), chunk.generation);
            chunk_targets.insert(target);
            if !identities.insert((
                chunk.unit_id.as_str(),
                chunk.generation,
                chunk.chunk.sequence,
            )) || !cursors.insert((
                chunk.unit_id.as_str(),
                chunk.generation,
                chunk.chunk.cursor.as_str(),
            )) || last_chunk_sequences
                .insert(target, chunk.chunk.sequence)
                .is_some_and(|sequence| sequence >= chunk.chunk.sequence)
            {
                return Err(
                    "node log batch contains conflicting or unordered chunk identities".into(),
                );
            }
            total_data_bytes = total_data_bytes
                .checked_add(chunk.chunk.data.len())
                .ok_or_else(|| "node log batch size overflowed".to_string())?;
        }
        let mut gap_identities = std::collections::BTreeSet::new();
        let mut last_gap_sequences = std::collections::BTreeMap::new();
        for gap in &self.gaps {
            gap.validate()?;
            let target = (gap.unit_id.as_str(), gap.generation);
            if chunk_targets.contains(&target) {
                return Err("node log batch mixes chunks and gaps for one target".into());
            }
            let reason = match gap.reason {
                RuntimeLogDiscontinuityReason::CursorLost => "cursor_lost",
                RuntimeLogDiscontinuityReason::SourceDisconnected => "source_disconnected",
            };
            if !identities.insert((gap.unit_id.as_str(), gap.generation, gap.sequence))
                || !gap_identities.insert((
                    gap.unit_id.as_str(),
                    gap.generation,
                    gap.cursor.as_deref(),
                    reason,
                ))
                || last_gap_sequences
                    .insert(target, gap.sequence)
                    .is_some_and(|sequence| sequence >= gap.sequence)
            {
                return Err(
                    "node log batch contains conflicting or unordered gap identities".into(),
                );
            }
        }
        if total_data_bytes > 16 * 1024 * 1024 {
            return Err("node log batch exceeds 16 MiB".into());
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| format!("could not encode node log batch: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLogChunkReceipt {
    pub schema: String,
    pub batch_id: Uuid,
    pub node_id: Uuid,
    pub accepted_chunks: u16,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub accepted_gaps: u16,
    pub replayed: bool,
}

impl NodeLogChunkReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-log-chunk-receipt.v1";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported node log chunk receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("batch_id", self.batch_id)?;
        validate_uuid("node_id", self.node_id)?;
        let accepted_records = usize::from(self.accepted_chunks) + usize::from(self.accepted_gaps);
        if accepted_records == 0 || accepted_records > 256 {
            return Err("node log receipt record count is invalid".into());
        }
        Ok(())
    }
}

const fn is_zero(value: &u16) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayAckState {
    Applied,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeGatewayAck {
    pub schema: String,
    pub acknowledgement_id: Uuid,
    pub command_id: Uuid,
    pub node_id: Uuid,
    pub revision: u64,
    pub snapshot_digest: String,
    pub state: GatewayAckState,
    pub message: Option<String>,
    pub acknowledged_at: DateTime<Utc>,
}

impl NodeGatewayAck {
    pub const SCHEMA: &'static str = "a3s.cloud.node-gateway-ack.v2";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!("unsupported Gateway ack schema {:?}", self.schema));
        }
        validate_uuid("acknowledgement_id", self.acknowledgement_id)?;
        validate_uuid("command_id", self.command_id)?;
        validate_uuid("node_id", self.node_id)?;
        if self.revision == 0 {
            return Err("Gateway acknowledgement revision must be positive".into());
        }
        validate_sha256("Gateway snapshot digest", &self.snapshot_digest)?;
        match (self.state, self.message.as_deref()) {
            (GatewayAckState::Applied, None) => {}
            (GatewayAckState::Rejected, Some(message)) => {
                validate_single_line("Gateway acknowledgement message", message, 16 * 1024)?;
            }
            (GatewayAckState::Applied, Some(_)) => {
                return Err("applied Gateway acknowledgement cannot contain a message".into())
            }
            (GatewayAckState::Rejected, None) => {
                return Err("rejected Gateway acknowledgement must contain a message".into())
            }
        }
        Ok(())
    }

    pub fn validate_for(
        &self,
        command_id: Uuid,
        node_id: Uuid,
        snapshot: &GatewaySnapshot,
    ) -> Result<(), String> {
        self.validate()?;
        snapshot.validate()?;
        if self.command_id != command_id
            || self.node_id != node_id
            || self.revision != snapshot.revision
            || self.snapshot_digest != snapshot.snapshot_digest
        {
            return Err(
                "Gateway acknowledgement does not match its command and exact snapshot revision"
                    .into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeGatewayAckReceipt {
    pub schema: String,
    pub acknowledgement_id: Uuid,
    pub command_id: Uuid,
    pub node_id: Uuid,
    pub replayed: bool,
}

impl NodeGatewayAckReceipt {
    pub const SCHEMA: &'static str = "a3s.cloud.node-gateway-ack-receipt.v2";

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway acknowledgement receipt schema {:?}",
                self.schema
            ));
        }
        validate_uuid("acknowledgement_id", self.acknowledgement_id)?;
        validate_uuid("command_id", self.command_id)?;
        validate_uuid("node_id", self.node_id)
    }
}
