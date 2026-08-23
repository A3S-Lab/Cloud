use crate::modules::edge::domain::events::{McpRoutePolicyChanged, McpRoutePolicyMutationKind};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, PrincipalId, ProjectId, RouteId,
    Sha256Digest,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use uuid::Uuid;

const CURSOR_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAuditCorrelation {
    Verified,
    Missing,
}

impl SecurityAuditCorrelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRoutePolicyTimelineEntry {
    pub event_id: Uuid,
    pub event_key: String,
    pub schema_version: u32,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: RouteId,
    pub policy_revision: u64,
    pub policy_digest: Sha256Digest,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub audit_correlation: SecurityAuditCorrelation,
    pub audit_record_id: Option<Uuid>,
    pub actor_principal_id: Option<PrincipalId>,
}

impl GatewayRoutePolicyTimelineEntry {
    pub fn from_owner_event(
        event: &DomainEventEnvelope,
        audit_record_id: Option<Uuid>,
        actor_principal_id: Option<Uuid>,
    ) -> Result<Self, String> {
        let payload = McpRoutePolicyChanged::decode_envelope(event)?;
        let audit_correlation = if audit_record_id.is_some() {
            SecurityAuditCorrelation::Verified
        } else {
            SecurityAuditCorrelation::Missing
        };
        let entry = Self {
            event_id: event.event_id,
            event_key: event.event_key.clone(),
            schema_version: event.schema_version,
            organization_id: OrganizationId::from_uuid(payload.organization_id),
            project_id: ProjectId::from_uuid(payload.project_id),
            environment_id: EnvironmentId::from_uuid(payload.environment_id),
            route_id: RouteId::from_uuid(payload.route_id),
            policy_revision: payload.policy_revision,
            policy_digest: Sha256Digest::parse(payload.policy_digest)?,
            occurred_at: event.occurred_at,
            correlation_id: event.correlation_id,
            audit_correlation,
            audit_record_id,
            actor_principal_id: actor_principal_id.map(PrincipalId::from_uuid),
        };
        entry.validate()?;
        Ok(entry)
    }

    pub fn validate(&self) -> Result<(), String> {
        let kind = McpRoutePolicyMutationKind::from_event_key(&self.event_key)?;
        let revision_shape_is_valid = match kind {
            McpRoutePolicyMutationKind::Create => self.policy_revision == 1,
            McpRoutePolicyMutationKind::Revise => self.policy_revision > 1,
        };
        let audit_shape_is_valid = match self.audit_correlation {
            SecurityAuditCorrelation::Verified => self
                .audit_record_id
                .is_some_and(|audit_id| !audit_id.is_nil()),
            SecurityAuditCorrelation::Missing => {
                self.audit_record_id.is_none() && self.actor_principal_id.is_none()
            }
        };
        if self.event_id.is_nil()
            || self.schema_version != 1
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.route_id.as_uuid().is_nil()
            || self.policy_revision == 0
            || !revision_shape_is_valid
            || canonical_timestamp(self.occurred_at) != self.occurred_at
            || self.correlation_id.is_nil()
            || self
                .actor_principal_id
                .is_some_and(|actor| actor.as_uuid().is_nil())
            || !audit_shape_is_valid
        {
            return Err("Gateway Route policy timeline entry is inconsistent".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayRoutePolicyTimelineCursor {
    pub occurred_at: DateTime<Utc>,
    pub event_id: Uuid,
}

impl GatewayRoutePolicyTimelineCursor {
    pub fn after(entry: &GatewayRoutePolicyTimelineEntry) -> Self {
        Self {
            occurred_at: entry.occurred_at,
            event_id: entry.event_id,
        }
    }

    pub fn encode(self) -> String {
        format!(
            "{CURSOR_VERSION}:{}:{}",
            self.occurred_at.timestamp_micros(),
            self.event_id
        )
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n']) {
            return Err("security timeline cursor is invalid".into());
        }
        let mut parts = value.split(':');
        let version = parts.next();
        let timestamp = parts.next();
        let event_id = parts.next();
        if version != Some(CURSOR_VERSION) || parts.next().is_some() {
            return Err("security timeline cursor is invalid".into());
        }
        let occurred_at = timestamp
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(DateTime::<Utc>::from_timestamp_micros)
            .ok_or_else(|| "security timeline cursor is invalid".to_owned())?;
        let event_id = event_id
            .and_then(|value| Uuid::parse_str(value).ok())
            .filter(|value| !value.is_nil())
            .ok_or_else(|| "security timeline cursor is invalid".to_owned())?;
        Ok(Self {
            occurred_at,
            event_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRoutePolicyTimelinePage {
    pub entries: Vec<GatewayRoutePolicyTimelineEntry>,
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::domain::events::MCP_ROUTE_POLICY_CREATED_EVENT_KEY;
    use chrono::TimeZone;

    #[test]
    fn cursor_round_trip_is_bounded_and_preserves_postgres_precision() {
        let cursor = GatewayRoutePolicyTimelineCursor {
            occurred_at: Utc
                .timestamp_opt(1_800_000_000, 123_456_000)
                .single()
                .expect("timestamp"),
            event_id: Uuid::now_v7(),
        };
        assert_eq!(
            GatewayRoutePolicyTimelineCursor::parse(&cursor.encode()),
            Ok(cursor)
        );
        for invalid in [
            "",
            "v2:1:00000000-0000-0000-0000-000000000001",
            "v1:nope:00000000-0000-0000-0000-000000000001",
            "v1:1:00000000-0000-0000-0000-000000000000",
            "v1:1:not-a-uuid",
            "v1:1:00000000-0000-0000-0000-000000000001:extra",
        ] {
            assert!(GatewayRoutePolicyTimelineCursor::parse(invalid).is_err());
        }
    }

    #[test]
    fn audit_gap_cannot_carry_an_unverified_actor_or_record() {
        let entry = GatewayRoutePolicyTimelineEntry {
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
            actor_principal_id: Some(PrincipalId::new()),
        };
        assert!(entry.validate().is_err());
    }
}
