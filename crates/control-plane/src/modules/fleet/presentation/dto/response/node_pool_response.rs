use crate::modules::fleet::application::NodePoolMutationResult;
use crate::modules::fleet::domain::entities::{NodePool, NodePoolMaintenanceWindow};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePoolResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub name: String,
    pub member_node_ids: Vec<Uuid>,
    pub maintenance: Option<NodePoolMaintenanceResponse>,
    pub spec_digest: String,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePoolMaintenanceResponse {
    pub generation: u64,
    pub target_node_ids: Vec<Uuid>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: String,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub status: String,
}

impl NodePoolResponse {
    pub fn new(node_pool: NodePool, evaluated_at: DateTime<Utc>, replayed: bool) -> Self {
        Self {
            id: node_pool.id.as_uuid(),
            organization_id: node_pool.organization_id.as_uuid(),
            name: node_pool.name.as_str().to_owned(),
            member_node_ids: node_pool
                .member_node_ids
                .into_iter()
                .map(|node_id| node_id.as_uuid())
                .collect(),
            maintenance: node_pool
                .maintenance
                .map(|window| NodePoolMaintenanceResponse::new(window, evaluated_at)),
            spec_digest: node_pool.spec_digest,
            aggregate_version: node_pool.aggregate_version,
            created_at: node_pool.created_at,
            updated_at: node_pool.updated_at,
            replayed,
        }
    }
}

impl NodePoolMaintenanceResponse {
    fn new(window: NodePoolMaintenanceWindow, evaluated_at: DateTime<Utc>) -> Self {
        Self {
            generation: window.generation,
            target_node_ids: window
                .target_node_ids
                .iter()
                .map(|node_id| node_id.as_uuid())
                .collect(),
            starts_at: window.starts_at,
            ends_at: window.ends_at,
            reason: window.reason.clone(),
            cancelled_at: window.cancelled_at,
            status: window.status_at(evaluated_at).as_str().into(),
        }
    }
}

impl From<NodePoolMutationResult> for NodePoolResponse {
    fn from(result: NodePoolMutationResult) -> Self {
        Self::new(result.node_pool, Utc::now(), result.replayed)
    }
}
