use super::OutboxMessage;
use crate::modules::shared_kernel::domain::ScopeContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const MAX_PORTABLE_VERSION: u64 = 9_007_199_254_740_991;

/// The one wire projection emitted by the transactional Outbox publisher.
///
/// `scope` is the canonical committed authority identity. `organizationId` is
/// retained only as a bounded rolling-upgrade projection for existing tenant
/// consumers and is rejected whenever it disagrees with that scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedOutboxEnvelope {
    scope: ScopeContext,
    organization_id: Option<Uuid>,
    aggregate_id: Uuid,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
    data: Value,
}

impl PublishedOutboxEnvelope {
    pub fn from_message(message: &OutboxMessage) -> Result<Self, String> {
        message.validate()?;
        let envelope = Self {
            scope: message.scope,
            organization_id: message.organization_id(),
            aggregate_id: message.aggregate_id,
            aggregate_version: message.aggregate_version,
            occurred_at: message.occurred_at,
            correlation_id: message.correlation_id,
            causation_id: message.causation_id,
            data: message.payload.clone(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        let canonical_organization_id = self
            .scope
            .organization_id()
            .map(|organization_id| organization_id.as_uuid());
        if self.organization_id != canonical_organization_id
            || self.aggregate_id.is_nil()
            || self.aggregate_version == 0
            || self.aggregate_version > MAX_PORTABLE_VERSION
            || self.correlation_id.is_nil()
            || self.causation_id.is_some_and(|value| value.is_nil())
            || !self.data.is_object()
        {
            return Err("published Outbox envelope is invalid".into());
        }
        Ok(())
    }

    pub const fn scope(&self) -> ScopeContext {
        self.scope
    }

    pub const fn organization_id(&self) -> Option<Uuid> {
        match self.scope.organization_id() {
            Some(value) => Some(value.as_uuid()),
            None => None,
        }
    }

    pub fn require_tenant_organization_id(&self) -> Result<Uuid, String> {
        self.validate()?;
        self.organization_id()
            .ok_or_else(|| "installation-scoped event has no tenant Organization".into())
    }

    pub const fn aggregate_id(&self) -> Uuid {
        self.aggregate_id
    }

    pub const fn aggregate_version(&self) -> u64 {
        self.aggregate_version
    }

    pub const fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    pub const fn correlation_id(&self) -> Uuid {
        self.correlation_id
    }

    pub const fn causation_id(&self) -> Option<Uuid> {
        self.causation_id
    }

    pub const fn data(&self) -> &Value {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::{InstallationId, OrganizationId};

    fn message(scope: ScopeContext) -> OutboxMessage {
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "notification.delivery.requested".into(),
            schema_version: 1,
            scope,
            aggregate_id: Uuid::now_v7(),
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: Some(Uuid::now_v7()),
            payload: serde_json::json!({"channel": "smtp"}),
            delivery_attempts: 0,
        }
    }

    #[test]
    fn organization_wire_projection_contains_one_canonical_scope_and_matching_legacy_id() {
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let envelope = PublishedOutboxEnvelope::from_message(&message(
            ScopeContext::organization(installation_id, organization_id).expect("scope"),
        ))
        .expect("published envelope");
        let value = serde_json::to_value(envelope).expect("wire JSON");

        assert_eq!(value["scope"]["kind"], "organization");
        assert_eq!(
            value["scope"]["installation_id"],
            installation_id.as_uuid().to_string()
        );
        assert_eq!(
            value["scope"]["organization_id"],
            organization_id.as_uuid().to_string()
        );
        assert_eq!(
            value["organizationId"],
            organization_id.as_uuid().to_string()
        );
    }

    #[test]
    fn installation_wire_projection_has_no_synthetic_organization() {
        let envelope = PublishedOutboxEnvelope::from_message(&message(
            ScopeContext::installation(InstallationId::new()).expect("scope"),
        ))
        .expect("published envelope");
        assert!(envelope.require_tenant_organization_id().is_err());
        let value = serde_json::to_value(envelope).expect("wire JSON");

        assert_eq!(value["scope"]["kind"], "installation");
        assert!(value["organizationId"].is_null());
    }

    #[test]
    fn mismatched_legacy_projection_and_unknown_extensions_fail_closed() {
        let envelope = PublishedOutboxEnvelope::from_message(&message(
            ScopeContext::organization(InstallationId::new(), OrganizationId::new())
                .expect("scope"),
        ))
        .expect("published envelope");
        let mut mismatched = serde_json::to_value(&envelope).expect("wire JSON");
        mismatched["organizationId"] = serde_json::json!(Uuid::now_v7());
        let mismatched: PublishedOutboxEnvelope =
            serde_json::from_value(mismatched).expect("envelope shape");
        assert!(mismatched.validate().is_err());

        let mut extended = serde_json::to_value(envelope).expect("wire JSON");
        extended["anotherScope"] = serde_json::json!(Uuid::now_v7());
        assert!(serde_json::from_value::<PublishedOutboxEnvelope>(extended).is_err());
    }
}
