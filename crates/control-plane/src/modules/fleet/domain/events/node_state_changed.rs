use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStateChanged {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub state: String,
}

impl NodeStateChanged {
    pub fn envelope(
        node: &Node,
        state: NodeState,
        changed_at: chrono::DateTime<chrono::Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "fleet.node.state-changed".into(),
            schema_version: 1,
            organization_id: node.organization_id.as_uuid(),
            aggregate_id: node.id.as_uuid(),
            aggregate_version: node.aggregate_version,
            occurred_at: changed_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: node.organization_id,
                node_id: node.id,
                state: state.as_str().into(),
            })?,
        })
    }
}
