use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::NodeCommandAck;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct AcknowledgeNodeCommand {
    pub authenticated_node_id: NodeId,
    pub acknowledgement: NodeCommandAck,
    pub received_at: DateTime<Utc>,
}

impl Command for AcknowledgeNodeCommand {
    type Output = ApplicationResult<AcknowledgeNodeCommandResult>;
}

#[derive(Debug, Clone)]
pub struct AcknowledgeNodeCommandResult {
    pub acknowledgement: NodeCommandAck,
    pub replayed: bool,
}
