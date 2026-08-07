use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::domain::canonical_timestamp;
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRoutePolicyMutationKind {
    Create,
    Revise,
}

impl McpRoutePolicyMutationKind {
    pub const fn event_key(self) -> &'static str {
        match self {
            Self::Create => "edge.mcp-route-policy.created",
            Self::Revise => "edge.mcp-route-policy.revised",
        }
    }

    pub const fn action(self) -> &'static str {
        self.event_key()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpRoutePolicyChanged {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub route_id: Uuid,
    pub policy_revision: u64,
    pub policy_digest: String,
}

impl McpRoutePolicyChanged {
    pub fn envelope(
        policy: &McpRoutePolicy,
        kind: McpRoutePolicyMutationKind,
        correlation_id: Uuid,
        occurred_at: DateTime<Utc>,
    ) -> Result<DomainEventEnvelope, String> {
        if correlation_id.is_nil() {
            return Err("MCP route policy correlation ID is invalid".into());
        }
        let spec = policy.spec();
        let payload = serde_json::to_value(Self {
            organization_id: spec.organization_id.as_uuid(),
            project_id: spec.project_id.as_uuid(),
            environment_id: spec.environment_id.as_uuid(),
            route_id: spec.route_id.as_uuid(),
            policy_revision: policy.policy_revision(),
            policy_digest: policy.policy_digest().to_string(),
        })
        .map_err(|error| error.to_string())?;
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: kind.event_key().into(),
            schema_version: 1,
            organization_id: spec.organization_id.as_uuid(),
            aggregate_id: spec.route_id.as_uuid(),
            aggregate_version: policy.policy_revision(),
            occurred_at: canonical_timestamp(occurred_at),
            correlation_id,
            causation_id: None,
            payload,
        })
    }
}
