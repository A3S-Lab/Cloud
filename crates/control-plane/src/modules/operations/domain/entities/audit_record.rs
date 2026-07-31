use crate::modules::shared_kernel::domain::{canonical_timestamp, AuditId, OrganizationId};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

const MAX_AUDIT_ACTION_BYTES: usize = 128;
const MAX_AUDIT_DETAILS_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct AuditRecord {
    pub id: AuditId,
    pub organization_id: OrganizationId,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub aggregate_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub request_id: Uuid,
    pub details: Value,
}

impl AuditRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: AuditId,
        organization_id: OrganizationId,
        actor_id: Option<Uuid>,
        action: impl Into<String>,
        aggregate_id: Uuid,
        occurred_at: DateTime<Utc>,
        request_id: Uuid,
        details: Value,
    ) -> Result<Self, String> {
        let record = Self {
            id,
            organization_id,
            actor_id,
            action: action.into(),
            aggregate_id,
            occurred_at,
            request_id,
            details,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.actor_id.is_some_and(|actor_id| actor_id.is_nil())
            || self.aggregate_id.is_nil()
            || self.request_id.is_nil()
        {
            return Err("audit identity must not be nil".into());
        }
        if self.action.is_empty()
            || self.action.len() > MAX_AUDIT_ACTION_BYTES
            || !self.action.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_' | b':')
            })
        {
            return Err("audit action is invalid".into());
        }
        if self.occurred_at != canonical_timestamp(self.occurred_at) {
            return Err("audit timestamp must use canonical precision".into());
        }
        if !self.details.is_object() {
            return Err("audit details must be an object".into());
        }
        let details_size = serde_json::to_vec(&self.details)
            .map_err(|_| "audit details could not be serialized".to_string())?
            .len();
        if details_size > MAX_AUDIT_DETAILS_BYTES {
            return Err("audit details exceed the 16 KiB limit".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};
    use serde_json::json;

    fn occurred_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
            .single()
            .expect("timestamp")
    }

    #[test]
    fn accepts_one_bounded_append_only_business_fact() {
        let record = AuditRecord::new(
            AuditId::new(),
            OrganizationId::new(),
            Some(Uuid::new_v4()),
            "edge.mcp-credential.issue",
            Uuid::new_v4(),
            occurred_at(),
            Uuid::new_v4(),
            json!({"result": "accepted"}),
        )
        .expect("audit record");

        assert_eq!(record.action, "edge.mcp-credential.issue");
        assert_eq!(record.details["result"], "accepted");
    }

    #[test]
    fn rejects_invalid_identity_action_time_and_unbounded_details() {
        let valid = || {
            (
                AuditId::new(),
                OrganizationId::new(),
                Some(Uuid::new_v4()),
                Uuid::new_v4(),
                Uuid::new_v4(),
            )
        };
        let (_, organization_id, actor_id, aggregate_id, request_id) = valid();
        assert!(AuditRecord::new(
            AuditId::from_uuid(Uuid::nil()),
            organization_id,
            actor_id,
            "edge.valid",
            aggregate_id,
            occurred_at(),
            request_id,
            json!({}),
        )
        .is_err());
        let (id, organization_id, actor_id, aggregate_id, request_id) = valid();
        assert!(AuditRecord::new(
            id,
            organization_id,
            actor_id,
            "Edge Invalid",
            aggregate_id,
            occurred_at(),
            request_id,
            json!({}),
        )
        .is_err());
        let (id, organization_id, actor_id, aggregate_id, request_id) = valid();
        let noncanonical = occurred_at()
            .with_nanosecond(1)
            .expect("nanosecond timestamp");
        assert!(AuditRecord::new(
            id,
            organization_id,
            actor_id,
            "edge.valid",
            aggregate_id,
            noncanonical,
            request_id,
            json!({}),
        )
        .is_err());
        let (id, organization_id, actor_id, aggregate_id, request_id) = valid();
        assert!(AuditRecord::new(
            id,
            organization_id,
            actor_id,
            "edge.valid",
            aggregate_id,
            occurred_at(),
            request_id,
            json!({"oversized": "x".repeat(MAX_AUDIT_DETAILS_BYTES)}),
        )
        .is_err());
    }
}
