use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeGatewayAck, NodeGatewayAckReceipt};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct RecordGatewayAcknowledgement {
    pub authenticated_node_id: NodeId,
    pub acknowledgement: NodeGatewayAck,
    pub received_at: DateTime<Utc>,
}

impl Command for RecordGatewayAcknowledgement {
    type Output = ApplicationResult<NodeGatewayAckReceipt>;
}
