use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeObservationBatch, NodeObservationReceipt};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct RecordNodeObservations {
    pub authenticated_node_id: NodeId,
    pub batch: NodeObservationBatch,
    pub received_at: DateTime<Utc>,
}

impl Command for RecordNodeObservations {
    type Output = ApplicationResult<NodeObservationReceipt>;
}
