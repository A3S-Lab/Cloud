use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::NodeId;
use a3s_boot::Command;
use a3s_cloud_contracts::{NodeSessionHello, NodeSessionSelection};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct NegotiateNodeSession {
    pub authenticated_node_id: NodeId,
    pub hello: NodeSessionHello,
    pub received_at: DateTime<Utc>,
}

impl Command for NegotiateNodeSession {
    type Output = ApplicationResult<NegotiateNodeSessionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiateNodeSessionResult {
    pub selection: NodeSessionSelection,
    pub replayed: bool,
}
