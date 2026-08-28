use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::CloudScopeRef;

const MAX_PORTABLE_VERSION: u64 = 9_007_199_254_740_991;
const MAX_EVENT_KEY_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainEventEnvelope {
    pub event_id: Uuid,
    pub event_key: String,
    pub schema_version: u32,
    pub scope: CloudScopeRef,
    pub aggregate_id: Uuid,
    pub aggregate_version: u64,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: Value,
}

impl DomainEventEnvelope {
    pub const fn organization_id(&self) -> Option<Uuid> {
        self.scope.organization_id()
    }

    pub fn require_tenant_organization_id(&self) -> Result<Uuid, String> {
        self.organization_id()
            .ok_or_else(|| "installation-scoped event has no tenant Organization".into())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        if self.event_id.is_nil()
            || !valid_event_key(&self.event_key)
            || self.schema_version == 0
            || self.aggregate_id.is_nil()
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_PORTABLE_VERSION
            || self.correlation_id.is_nil()
            || self.causation_id.is_some_and(|value| value.is_nil())
            || !self.payload.is_object()
        {
            return Err("domain event envelope is invalid".into());
        }
        Ok(())
    }
}

fn valid_event_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EVENT_KEY_BYTES
        && value.split('.').count() >= 3
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> DomainEventEnvelope {
        DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.platform-role.changed".into(),
            schema_version: 1,
            scope: CloudScopeRef::installation(Uuid::now_v7()).expect("scope"),
            aggregate_id: Uuid::now_v7(),
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({"changed": true}),
        }
    }

    #[test]
    fn envelope_accepts_one_valid_closed_scope_and_rejects_forged_identity() {
        let value = envelope();
        assert_eq!(value.validate(), Ok(()));

        let mut forged = value;
        forged.scope = CloudScopeRef::Organization {
            organization_id: Uuid::nil(),
        };
        assert!(forged.validate().is_err());
    }
}
