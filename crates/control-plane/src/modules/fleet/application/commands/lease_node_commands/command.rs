use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeCommandLeaseRequest, NodeCommandLeaseResponse};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct LeaseNodeCommands {
    pub authenticated_node_id: NodeId,
    pub request: NodeCommandLeaseRequest,
    pub received_at: DateTime<Utc>,
}

impl Command for LeaseNodeCommands {
    type Output = ApplicationResult<NodeCommandLeaseResponse>;
}
