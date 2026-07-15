use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ChangeNodeState {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub state: NodeState,
    pub expected_version: u64,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for ChangeNodeState {
    type Output = ApplicationResult<ChangeNodeStateResult>;
}

#[derive(Debug, Clone)]
pub struct ChangeNodeStateResult {
    pub node: Node,
    pub replayed: bool,
}
