use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, NodeId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayScope {
    pub id: GatewayScopeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub node_id: NodeId,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GatewayScope {
    pub fn create(
        id: GatewayScopeId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
        created_at: DateTime<Utc>,
    ) -> Self {
        let created_at = canonical_timestamp(created_at);
        Self {
            id,
            organization_id,
            project_id,
            environment_id,
            node_id,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn owns(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
    ) -> bool {
        self.organization_id == organization_id
            && self.project_id == project_id
            && self.environment_id == environment_id
            && self.node_id == node_id
    }
}
