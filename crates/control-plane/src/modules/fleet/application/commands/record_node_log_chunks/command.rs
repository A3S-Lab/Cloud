use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeLogChunkBatch, NodeLogChunkReceipt};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct RecordNodeLogChunks {
    pub authenticated_node_id: NodeId,
    pub batch: NodeLogChunkBatch,
    pub received_at: DateTime<Utc>,
}

impl Command for RecordNodeLogChunks {
    type Output = ApplicationResult<NodeLogChunkReceipt>;
}
