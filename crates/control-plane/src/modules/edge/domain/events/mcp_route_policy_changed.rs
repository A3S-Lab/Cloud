use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::domain::{canonical_timestamp, Sha256Digest};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MCP_ROUTE_POLICY_CREATED_EVENT_KEY: &str = "edge.mcp-route-policy.created";
pub const MCP_ROUTE_POLICY_REVISED_EVENT_KEY: &str = "edge.mcp-route-policy.revised";
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpRoutePolicyMutationKind {
    Create,
    Revise,
}

impl McpRoutePolicyMutationKind {
    pub const fn event_key(self) -> &'static str {
        match self {
            Self::Create => MCP_ROUTE_POLICY_CREATED_EVENT_KEY,
            Self::Revise => MCP_ROUTE_POLICY_REVISED_EVENT_KEY,
        }
    }

    pub const fn action(self) -> &'static str {
        self.event_key()
    }

    pub fn from_event_key(event_key: &str) -> Result<Self, String> {
        match event_key {
            MCP_ROUTE_POLICY_CREATED_EVENT_KEY => Ok(Self::Create),
            MCP_ROUTE_POLICY_REVISED_EVENT_KEY => Ok(Self::Revise),
            _ => Err("MCP route policy event key is unsupported".into()),
        }
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
        let event = DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: kind.event_key().into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: spec.organization_id.as_uuid(),
            },
            aggregate_id: spec.route_id.as_uuid(),
            aggregate_version: policy.policy_revision(),
            occurred_at: canonical_timestamp(occurred_at),
            correlation_id,
            causation_id: None,
            payload,
        };
        Self::decode_envelope(&event)?;
        Ok(event)
    }

    pub fn decode_envelope(event: &DomainEventEnvelope) -> Result<Self, String> {
        let kind = McpRoutePolicyMutationKind::from_event_key(&event.event_key)?;
        let payload: Self = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("MCP route policy payload is invalid: {error}"))?;
        let revision_shape_is_valid = match kind {
            McpRoutePolicyMutationKind::Create => payload.policy_revision == 1,
            McpRoutePolicyMutationKind::Revise => payload.policy_revision > 1,
        };
        if event.schema_version != 1
            || event.event_id.is_nil()
            || event.organization_id().is_none()
            || event.aggregate_id.is_nil()
            || event.correlation_id.is_nil()
            || event.causation_id.is_some()
            || canonical_timestamp(event.occurred_at) != event.occurred_at
            || payload.organization_id.is_nil()
            || payload.project_id.is_nil()
            || payload.environment_id.is_nil()
            || payload.route_id.is_nil()
            || Some(payload.organization_id) != event.organization_id()
            || payload.route_id != event.aggregate_id
            || payload.policy_revision == 0
            || payload.policy_revision > MAX_SAFE_ACL_INTEGER
            || payload.policy_revision != event.aggregate_version
            || !revision_shape_is_valid
            || Sha256Digest::parse(payload.policy_digest.as_str()).is_err()
        {
            return Err("MCP route policy event identity is inconsistent".into());
        }
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: McpRoutePolicyMutationKind) -> DomainEventEnvelope {
        let organization_id = Uuid::now_v7();
        let route_id = Uuid::now_v7();
        let policy_revision = match kind {
            McpRoutePolicyMutationKind::Create => 1,
            McpRoutePolicyMutationKind::Revise => 2,
        };
        DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: kind.event_key().into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization { organization_id },
            aggregate_id: route_id,
            aggregate_version: policy_revision,
            occurred_at: canonical_timestamp(Utc::now()),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({
                "organization_id": organization_id,
                "project_id": Uuid::now_v7(),
                "environment_id": Uuid::now_v7(),
                "route_id": route_id,
                "policy_revision": policy_revision,
                "policy_digest": format!("sha256:{}", "a".repeat(64)),
            }),
        }
    }

    #[test]
    fn decoder_accepts_only_closed_created_and_revised_owner_facts() {
        for kind in [
            McpRoutePolicyMutationKind::Create,
            McpRoutePolicyMutationKind::Revise,
        ] {
            let event = event(kind);
            let payload = McpRoutePolicyChanged::decode_envelope(&event).expect("owner fact");
            assert_eq!(Some(payload.organization_id), event.organization_id());
            assert_eq!(payload.route_id, event.aggregate_id);
            assert_eq!(payload.policy_revision, event.aggregate_version);
        }

        let mut invalid = event(McpRoutePolicyMutationKind::Create);
        for mutate in [
            |event: &mut DomainEventEnvelope| event.event_key = "edge.route.created".into(),
            |event: &mut DomainEventEnvelope| event.schema_version = 2,
            |event: &mut DomainEventEnvelope| event.aggregate_version = 2,
            |event: &mut DomainEventEnvelope| event.correlation_id = Uuid::nil(),
            |event: &mut DomainEventEnvelope| event.causation_id = Some(Uuid::now_v7()),
        ] {
            let mut candidate = invalid.clone();
            mutate(&mut candidate);
            assert!(McpRoutePolicyChanged::decode_envelope(&candidate).is_err());
        }
        invalid.payload["private_error"] = serde_json::json!("must stay private");
        assert!(McpRoutePolicyChanged::decode_envelope(&invalid).is_err());
    }

    #[test]
    fn decoder_rejects_payload_identity_revision_and_digest_drift() {
        let base = event(McpRoutePolicyMutationKind::Revise);
        for (field, value) in [
            ("organization_id", serde_json::json!(Uuid::now_v7())),
            ("project_id", serde_json::json!(Uuid::nil())),
            ("environment_id", serde_json::json!(Uuid::nil())),
            ("route_id", serde_json::json!(Uuid::now_v7())),
            ("policy_revision", serde_json::json!(1)),
            (
                "policy_digest",
                serde_json::json!(format!("sha256:{}", "A".repeat(64))),
            ),
        ] {
            let mut candidate = base.clone();
            candidate.payload[field] = value;
            assert!(
                McpRoutePolicyChanged::decode_envelope(&candidate).is_err(),
                "accepted drifted {field}"
            );
        }
    }
}
