use crate::modules::edge::domain::{GatewayRollout, GatewayRolloutPolicy, GatewayScope};
use crate::modules::shared_kernel::domain::{GatewayRolloutId, GatewayScopeId, NodeId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRolloutStaged {
    pub gateway_rollout_id: GatewayRolloutId,
    pub gateway_scope_id: GatewayScopeId,
    pub membership_generation: u64,
    pub rollout_generation: u64,
    pub member_node_ids: Vec<NodeId>,
    pub policy: GatewayRolloutPolicy,
}

impl GatewayRolloutStaged {
    pub fn envelope(
        scope: &GatewayScope,
        rollout: &GatewayRollout,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: uuid::Uuid::now_v7(),
            event_key: "edge.gateway-rollout.staged".into(),
            schema_version: 1,
            organization_id: scope.organization_id.as_uuid(),
            aggregate_id: rollout.id.as_uuid(),
            aggregate_version: rollout.aggregate_version,
            occurred_at: rollout.started_at,
            correlation_id: rollout.correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                gateway_rollout_id: rollout.id,
                gateway_scope_id: rollout.gateway_scope_id,
                membership_generation: rollout.membership_generation,
                rollout_generation: rollout.generation,
                member_node_ids: rollout
                    .replicas
                    .iter()
                    .map(|replica| replica.node_id)
                    .collect(),
                policy: rollout.policy,
            })?,
        })
    }
}
