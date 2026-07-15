use crate::modules::fleet::domain::entities::{NodeCommand, NodeCommandDraft};
use crate::modules::shared_kernel::domain::{IdempotentWrite, NodeId, RepositoryError};
use a3s_cloud_contracts::{
    NodeCommandAck, NodeCommandLeaseRequest, NodeCommandLeaseResponse, NodeGatewayAck,
    NodeGatewayAckReceipt, NodeLogChunkReceipt, NodeObservationBatch, NodeObservationReceipt,
};
use a3s_runtime::contract::RuntimeObservation;
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
        if self.batch_id.is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.chunks.is_empty()
            || self.chunks.len() > 256
            || !is_sha256(&self.payload_digest)
        {
            return Err("log receipt batch is invalid".into());
        }
        let mut identities = std::collections::BTreeSet::new();
        let mut cursors = std::collections::BTreeSet::new();
        for chunk in &self.chunks {
            chunk.validate()?;
            if !identities.insert((chunk.unit_id.as_str(), chunk.generation, chunk.sequence))
                || !cursors.insert((
                    chunk.unit_id.as_str(),
                    chunk.generation,
                    chunk.cursor.as_str(),
                ))
            {
                return Err("log receipt batch contains duplicate sequence or cursor".into());
            }
        }
        Ok(())
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
}

fn is_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
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

    async fn record_log_chunks(
        &self,
        batch: NodeLogBatchReceiptDraft,
        received_at: DateTime<Utc>,
    ) -> Result<NodeLogChunkReceipt, RepositoryError>;
}
