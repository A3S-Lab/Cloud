use crate::modules::edge::domain::GatewayScope;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayScopeResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub node_id: Uuid,
    pub member_node_ids: Vec<Uuid>,
    pub membership_generation: u64,
    pub min_ready: u32,
    pub max_unavailable: u32,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<GatewayScope> for GatewayScopeResponse {
    fn from(scope: GatewayScope) -> Self {
        Self {
            id: scope.id.as_uuid(),
            organization_id: scope.organization_id.as_uuid(),
            project_id: scope.project_id.as_uuid(),
            environment_id: scope.environment_id.as_uuid(),
            node_id: scope.node_id.as_uuid(),
            member_node_ids: scope
                .member_node_ids
                .into_iter()
                .map(|node_id| node_id.as_uuid())
                .collect(),
            membership_generation: scope.membership_generation,
            min_ready: scope.rollout_policy.min_ready,
            max_unavailable: scope.rollout_policy.max_unavailable,
            aggregate_version: scope.aggregate_version,
            created_at: scope.created_at,
            updated_at: scope.updated_at,
        }
    }
}
