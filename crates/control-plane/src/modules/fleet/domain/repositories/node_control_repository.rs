use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::fleet::domain::repositories::NodeLogCompactionRange;
use crate::modules::shared_kernel::domain::{IdempotentWrite, NodeId, RepositoryError};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeLogChunkReceipt, NodeObservationBatch, NodeObservationReceipt,
};
use a3s_runtime::contract::{RuntimeLogDiscontinuityReason, RuntimeLogStream, RuntimeObservation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogBatchReceiptDraft {
    pub batch_id: Uuid,
    pub node_id: NodeId,
    pub payload_digest: String,
    pub sent_at: DateTime<Utc>,
    pub chunks: Vec<NodeLogChunkReceiptDraft>,
    pub gaps: Vec<NodeLogGapReceiptDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogBatchReplay {
    pub batch_id: Uuid,
    pub node_id: NodeId,
    pub payload_digest: String,
    pub sent_at: DateTime<Utc>,
    pub chunk_count: u16,
    pub gap_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogChunkReceiptDraft {
    pub unit_id: String,
    pub generation: u64,
    pub cursor: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub stream: String,
    pub checksum: String,
    pub object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogGapReceiptDraft {
    pub unit_id: String,
    pub generation: u64,
    pub cursor: Option<String>,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub reason: RuntimeLogDiscontinuityReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogChunkQuery {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub after_sequence: Option<u64>,
    pub limit: usize,
    pub stream: Option<RuntimeLogStream>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogChunkMetadata {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub cursor: String,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub stream: RuntimeLogStream,
    pub checksum: String,
    pub object_key: String,
    pub retained_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLogGapMetadata {
    pub node_id: NodeId,
    pub unit_id: String,
    pub generation: u64,
    pub cursor: Option<String>,
    pub sequence: u64,
    pub observed_at_ms: u64,
    pub reason: RuntimeLogDiscontinuityReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeObservationRecord {
    pub report_id: Uuid,
    pub node_id: NodeId,
    pub command_id: Option<crate::modules::shared_kernel::domain::NodeCommandId>,
    pub observed_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

impl NodeLogBatchReceiptDraft {
    pub fn validate(&self) -> Result<(), String> {
        let record_count = self
            .chunks
            .len()
            .checked_add(self.gaps.len())
            .ok_or_else(|| "log receipt batch record count overflowed".to_string())?;
        if self.batch_id.is_nil()
            || self.node_id.as_uuid().is_nil()
            || record_count == 0
            || record_count > 256
            || !is_sha256(&self.payload_digest)
        {
            return Err("log receipt batch is invalid".into());
        }
        let mut identities = std::collections::BTreeSet::new();
        let mut cursors = std::collections::BTreeSet::new();
        let mut last_sequences = std::collections::BTreeMap::new();
        let mut chunk_targets = std::collections::BTreeSet::new();
        for chunk in &self.chunks {
            chunk.validate()?;
            chunk_targets.insert((chunk.unit_id.as_str(), chunk.generation));
            if !identities.insert((chunk.unit_id.as_str(), chunk.generation, chunk.sequence))
                || !cursors.insert((
                    chunk.unit_id.as_str(),
                    chunk.generation,
                    chunk.cursor.as_str(),
                ))
            {
                return Err("log receipt batch contains duplicate sequence or cursor".into());
            }
            let identity = (chunk.unit_id.as_str(), chunk.generation);
            if last_sequences
                .insert(identity, chunk.sequence)
                .is_some_and(|sequence| sequence >= chunk.sequence)
            {
                return Err("log receipt batch sequences must strictly advance".into());
            }
        }
        let mut gap_identities = std::collections::BTreeSet::new();
        let mut last_gap_sequences = std::collections::BTreeMap::new();
        for gap in &self.gaps {
            gap.validate()?;
            let target = (gap.unit_id.as_str(), gap.generation);
            if chunk_targets.contains(&target)
                || !identities.insert((gap.unit_id.as_str(), gap.generation, gap.sequence))
                || !gap_identities.insert((
                    gap.unit_id.as_str(),
                    gap.generation,
                    gap.cursor.as_deref(),
                    gap_reason_name(gap.reason),
                ))
                || last_gap_sequences
                    .insert(target, gap.sequence)
                    .is_some_and(|sequence| sequence >= gap.sequence)
            {
                return Err("log receipt batch contains conflicting gap identities".into());
            }
        }
        Ok(())
    }
}

impl NodeLogBatchReplay {
    pub fn validate(&self) -> Result<(), String> {
        if self.batch_id.is_nil()
            || self.node_id.as_uuid().is_nil()
            || !is_sha256(&self.payload_digest)
            || usize::from(self.chunk_count) + usize::from(self.gap_count) == 0
            || usize::from(self.chunk_count) + usize::from(self.gap_count) > 256
        {
            return Err("log batch replay identity is invalid".into());
        }
        Ok(())
    }

    pub fn receipt(&self) -> NodeLogChunkReceipt {
        NodeLogChunkReceipt {
            schema: NodeLogChunkReceipt::SCHEMA.into(),
            batch_id: self.batch_id,
            node_id: self.node_id.as_uuid(),
            accepted_chunks: self.chunk_count,
            accepted_gaps: self.gap_count,
            replayed: true,
        }
    }
}

impl NodeLogGapReceiptDraft {
    fn validate(&self) -> Result<(), String> {
        if self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.cursor.as_ref().is_some_and(|cursor| {
                cursor.is_empty() || cursor.len() > 1024 || cursor.contains('\0')
            })
        {
            return Err("log gap receipt is invalid".into());
        }
        if self.reason == RuntimeLogDiscontinuityReason::CursorLost && self.cursor.is_none() {
            return Err("cursor-loss log gap must bind the lost cursor".into());
        }
        Ok(())
    }

    pub fn metadata(&self, node_id: NodeId) -> Result<NodeLogGapMetadata, String> {
        self.validate()?;
        Ok(NodeLogGapMetadata {
            node_id,
            unit_id: self.unit_id.clone(),
            generation: self.generation,
            cursor: self.cursor.clone(),
            sequence: self.sequence,
            observed_at_ms: self.observed_at_ms,
            reason: self.reason,
        })
    }
}

impl NodeLogChunkReceiptDraft {
    fn validate(&self) -> Result<(), String> {
        if self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.cursor.is_empty()
            || self.cursor.len() > 1024
            || self.cursor.contains('\0')
            || !matches!(self.stream.as_str(), "stdout" | "stderr")
            || !is_sha256(&self.checksum)
            || self.object_key.is_empty()
            || self.object_key.len() > 4096
        {
            return Err("log chunk receipt is invalid".into());
        }
        Ok(())
    }

    pub fn metadata(
        &self,
        node_id: NodeId,
        retained_at: Option<DateTime<Utc>>,
    ) -> Result<NodeLogChunkMetadata, String> {
        self.validate()?;
        let stream = match self.stream.as_str() {
            "stdout" => RuntimeLogStream::Stdout,
            "stderr" => RuntimeLogStream::Stderr,
            _ => return Err("log chunk receipt stream is invalid".into()),
        };
        Ok(NodeLogChunkMetadata {
            node_id,
            unit_id: self.unit_id.clone(),
            generation: self.generation,
            cursor: self.cursor.clone(),
            sequence: self.sequence,
            observed_at_ms: self.observed_at_ms,
            stream,
            checksum: self.checksum.clone(),
            object_key: self.object_key.clone(),
            retained_at,
        })
    }
}

impl NodeLogChunkQuery {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.unit_id.is_empty()
            || self.unit_id.len() > 512
            || self.unit_id.contains('\0')
            || self.generation == 0
            || self.limit == 0
            || self.limit > 1_000
        {
            return Err("log chunk query is invalid".into());
        }
        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

const fn gap_reason_name(reason: RuntimeLogDiscontinuityReason) -> &'static str {
    match reason {
        RuntimeLogDiscontinuityReason::CursorLost => "cursor_lost",
        RuntimeLogDiscontinuityReason::SourceDisconnected => "source_disconnected",
    }
}

#[async_trait]
pub trait INodeControlRepository: Send + Sync {
    async fn enqueue_command(
        &self,
        draft: NodeCommandDraft,
    ) -> Result<IdempotentWrite<NodeCommand>, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
    ) -> Result<Option<NodeCommand>, RepositoryError>;

    async fn lease_commands(
        &self,
        request: &NodeCommandLeaseRequest,
        lease_id: Uuid,
        now: DateTime<Utc>,
        leased_until: DateTime<Utc>,
    ) -> Result<NodeCommandLeaseResponse, RepositoryError>;

    async fn acknowledge_command(
        &self,
        acknowledgement: NodeCommandAck,
        received_at: DateTime<Utc>,
    ) -> Result<IdempotentWrite<NodeCommandAck>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: crate::modules::shared_kernel::domain::NodeCommandId,
    ) -> Result<Option<NodeCommandAck>, RepositoryError>;

    async fn record_observations(
        &self,
        batch: NodeObservationBatch,
        received_at: DateTime<Utc>,
    ) -> Result<NodeObservationReceipt, RepositoryError>;

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<RuntimeObservationRecord>, RepositoryError>;

    async fn record_gateway_acknowledgement(
        &self,
        acknowledgement: NodeGatewayAck,
        received_at: DateTime<Utc>,
    ) -> Result<NodeGatewayAckReceipt, RepositoryError>;

    async fn replay_log_batch(
        &self,
        batch: NodeLogBatchReplay,
    ) -> Result<Option<NodeLogChunkReceipt>, RepositoryError>;

    async fn record_log_chunks(
        &self,
        batch: NodeLogBatchReceiptDraft,
        received_at: DateTime<Utc>,
    ) -> Result<NodeLogChunkReceipt, RepositoryError>;

    async fn list_log_chunks(
        &self,
        query: NodeLogChunkQuery,
    ) -> Result<Vec<NodeLogChunkMetadata>, RepositoryError>;

    async fn list_log_gaps(
        &self,
        query: NodeLogChunkQuery,
    ) -> Result<Vec<NodeLogGapMetadata>, RepositoryError>;

    async fn list_log_compaction_ranges(
        &self,
        query: NodeLogChunkQuery,
    ) -> Result<Vec<NodeLogCompactionRange>, RepositoryError>;
}
