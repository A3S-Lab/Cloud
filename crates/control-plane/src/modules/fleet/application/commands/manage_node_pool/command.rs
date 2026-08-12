use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{NodeId, NodePoolId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum NodePoolMutation {
    Create {
        name: String,
        member_node_ids: Vec<NodeId>,
    },
    AddMembers {
        expected_version: u64,
        member_node_ids: Vec<NodeId>,
    },
    RequestMemberRemoval {
        expected_version: u64,
        member_node_ids: Vec<NodeId>,
    },
    ScheduleMaintenance {
        expected_version: u64,
        target_node_ids: Vec<NodeId>,
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        reason: String,
    },
    CancelMaintenance {
        expected_version: u64,
        maintenance_generation: u64,
    },
}

#[derive(Debug, Clone)]
pub struct ManageNodePool {
    pub organization_id: OrganizationId,
    pub node_pool_id: NodePoolId,
    pub mutation: NodePoolMutation,
    pub resource_access: ResourceAccessEvaluator,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for ManageNodePool {
    type Output = ApplicationResult<NodePoolMutationResult>;
}

#[derive(Debug, Clone)]
pub struct NodePoolMutationResult {
    pub node_pool: NodePool,
    pub replayed: bool,
}
