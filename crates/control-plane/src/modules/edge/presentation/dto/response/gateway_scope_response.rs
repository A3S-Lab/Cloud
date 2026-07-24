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
            aggregate_version: scope.aggregate_version,
            created_at: scope.created_at,
            updated_at: scope.updated_at,
        }
    }
}
