use crate::modules::security::domain::{
    GatewayRoutePolicyTimelineEntry, GatewayRoutePolicyTimelinePage,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRoutePolicyTimelineEntryResponse {
    pub event_id: Uuid,
    pub event_key: String,
    pub schema_version: u32,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub route_id: Uuid,
    pub policy_revision: u64,
    pub policy_digest: String,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub audit_correlation: &'static str,
    pub audit_record_id: Option<Uuid>,
    pub actor_principal_id: Option<Uuid>,
}

impl From<GatewayRoutePolicyTimelineEntry> for GatewayRoutePolicyTimelineEntryResponse {
    fn from(entry: GatewayRoutePolicyTimelineEntry) -> Self {
        Self {
            event_id: entry.event_id,
            event_key: entry.event_key,
            schema_version: entry.schema_version,
            organization_id: entry.organization_id.as_uuid(),
            project_id: entry.project_id.as_uuid(),
            environment_id: entry.environment_id.as_uuid(),
            route_id: entry.route_id.as_uuid(),
            policy_revision: entry.policy_revision,
            policy_digest: entry.policy_digest.to_string(),
            occurred_at: entry.occurred_at,
            correlation_id: entry.correlation_id,
            audit_correlation: entry.audit_correlation.as_str(),
            audit_record_id: entry.audit_record_id,
            actor_principal_id: entry.actor_principal_id.map(|value| value.as_uuid()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRoutePolicyTimelinePageResponse {
    pub entries: Vec<GatewayRoutePolicyTimelineEntryResponse>,
    pub next_cursor: Option<String>,
}

impl From<GatewayRoutePolicyTimelinePage> for GatewayRoutePolicyTimelinePageResponse {
    fn from(page: GatewayRoutePolicyTimelinePage) -> Self {
        Self {
            entries: page
                .entries
                .into_iter()
                .map(GatewayRoutePolicyTimelineEntryResponse::from)
                .collect(),
            next_cursor: page.next_cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::events::MCP_ROUTE_POLICY_CREATED_EVENT_KEY;
    use crate::modules::security::domain::SecurityAuditCorrelation;
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, RouteId, Sha256Digest,
    };

    #[test]
    fn response_is_typed_and_never_exposes_owner_payload_or_audit_details() {
        let response =
            GatewayRoutePolicyTimelineEntryResponse::from(GatewayRoutePolicyTimelineEntry {
                event_id: Uuid::now_v7(),
                event_key: MCP_ROUTE_POLICY_CREATED_EVENT_KEY.into(),
                schema_version: 1,
                organization_id: OrganizationId::new(),
                project_id: ProjectId::new(),
                environment_id: EnvironmentId::new(),
                route_id: RouteId::new(),
                policy_revision: 1,
                policy_digest: Sha256Digest::parse(format!("sha256:{}", "a".repeat(64)))
                    .expect("digest"),
                occurred_at: canonical_timestamp(Utc::now()),
                correlation_id: Uuid::now_v7(),
                audit_correlation: SecurityAuditCorrelation::Missing,
                audit_record_id: None,
                actor_principal_id: None,
            });
        let value = serde_json::to_value(response).expect("timeline response");
        assert_eq!(value.as_object().map(serde_json::Map::len), Some(14));
        for forbidden in ["payload", "details", "canonicalAcl", "privateError"] {
            assert!(!value.as_object().expect("object").contains_key(forbidden));
        }
    }
}
