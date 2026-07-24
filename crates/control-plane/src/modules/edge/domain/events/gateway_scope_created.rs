use crate::modules::edge::domain::GatewayScope;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GatewayScopeId, NodeId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayScopeCreated {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub gateway_scope_id: GatewayScopeId,
    pub node_id: NodeId,
}

impl GatewayScopeCreated {
    pub fn envelope(
        scope: &GatewayScope,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "edge.gateway-scope.created".into(),
            schema_version: 1,
            organization_id: scope.organization_id.as_uuid(),
            aggregate_id: scope.id.as_uuid(),
            aggregate_version: scope.aggregate_version,
            occurred_at: scope.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.environment_id,
                gateway_scope_id: scope.id,
                node_id: scope.node_id,
            })?,
        })
    }
}
