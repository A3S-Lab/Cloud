use crate::modules::fleet::application::{ChangeNodeStateResult, NodeQueryResult};
use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::NodeAvailability;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub state: String,
    pub availability: String,
    pub agent_instance_id: Uuid,
    pub agent_version: String,
    pub runtime_provider_id: String,
    pub runtime_provider_build: String,
    pub capabilities_digest: String,
    pub capabilities: Value,
    pub enrolled_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub aggregate_version: u64,
    pub replayed: bool,
}

impl NodeResponse {
    pub fn new(node: Node, availability: NodeAvailability, replayed: bool) -> Self {
        Self {
            id: node.id.as_uuid(),
            organization_id: node.organization_id.as_uuid(),
            name: node.name.value().to_owned(),
            state: node.state.as_str().into(),
            availability: availability.as_str().into(),
            agent_instance_id: node.agent_instance_id,
            agent_version: node.agent_version,
            runtime_provider_id: node.capabilities.provider_id().into(),
            runtime_provider_build: node.capabilities.provider_build().into(),
            capabilities_digest: node.capabilities.digest().into(),
            capabilities: node.capabilities.document().clone(),
            enrolled_at: node.enrolled_at,
            last_observed_at: node.last_observed_at,
            aggregate_version: node.aggregate_version,
            replayed,
        }
    }
}

impl From<NodeQueryResult> for NodeResponse {
    fn from(result: NodeQueryResult) -> Self {
        Self::new(result.node, result.availability, false)
    }
}

impl From<(ChangeNodeStateResult, NodeAvailability)> for NodeResponse {
    fn from((result, availability): (ChangeNodeStateResult, NodeAvailability)) -> Self {
        Self::new(result.node, availability, result.replayed)
    }
}
