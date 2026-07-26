use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeResourceInventory, NodeResourceInventoryReceipt};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct RecordNodeResourceInventory {
    pub authenticated_node_id: NodeId,
    pub inventory: NodeResourceInventory,
    pub received_at: DateTime<Utc>,
}

impl Command for RecordNodeResourceInventory {
    type Output = ApplicationResult<NodeResourceInventoryReceipt>;
}
