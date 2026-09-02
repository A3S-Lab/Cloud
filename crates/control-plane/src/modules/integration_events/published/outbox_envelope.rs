use crate::modules::shared_kernel::domain::ScopeContext;
use a3s_cloud_contracts::DomainEventEnvelope;
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
    pub(in crate::modules::integration_events) fn from_committed_event(
        scope: ScopeContext,
        event: DomainEventEnvelope,
    ) -> Result<Self, String> {
        scope.validate()?;
        event.validate()?;
        if event.scope != scope.reference() {
            return Err("published Outbox event does not match its committed scope".into());
        }
        let envelope = Self {
            scope,
            organization_id: scope
                .organization_id()
                .map(|organization_id| organization_id.as_uuid()),
            aggregate_id: event.aggregate_id,
            aggregate_version: event.aggregate_version,
            occurred_at: event.occurred_at,
            correlation_id: event.correlation_id,
            causation_id: event.causation_id,
            data: event.payload,
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

    fn event(scope: ScopeContext) -> DomainEventEnvelope {
        DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "notification.delivery.requested".into(),
            schema_version: 1,
            scope: scope.reference(),
            aggregate_id: Uuid::now_v7(),
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: Some(Uuid::now_v7()),
            payload: serde_json::json!({"channel": "smtp"}),
        }
    }

    fn envelope(scope: ScopeContext) -> PublishedOutboxEnvelope {
        PublishedOutboxEnvelope::from_committed_event(scope, event(scope))
            .expect("published envelope")
    }

    #[test]
    fn organization_wire_projection_contains_one_canonical_scope_and_matching_legacy_id() {
        let installation_id = InstallationId::new();
        let organization_id = OrganizationId::new();
        let envelope =
            envelope(ScopeContext::organization(installation_id, organization_id).expect("scope"));
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
        let envelope = envelope(ScopeContext::installation(InstallationId::new()).expect("scope"));
        assert!(envelope.require_tenant_organization_id().is_err());
        let value = serde_json::to_value(envelope).expect("wire JSON");

        assert_eq!(value["scope"]["kind"], "installation");
        assert!(value["organizationId"].is_null());
    }

    #[test]
    fn owner_domain_validation_rejects_an_invalid_event_key_before_publication() {
        let scope = ScopeContext::installation(InstallationId::new()).expect("installation scope");
        let mut invalid = event(scope);
        invalid.event_key = "notification.*.requested".into();

        assert!(PublishedOutboxEnvelope::from_committed_event(scope, invalid).is_err());
    }

    #[test]
    fn committed_scope_and_owner_event_scope_must_match() {
        let committed = ScopeContext::installation(InstallationId::new()).expect("committed scope");
        let another = ScopeContext::installation(InstallationId::new()).expect("another scope");

        assert!(PublishedOutboxEnvelope::from_committed_event(committed, event(another)).is_err());
    }

    #[test]
    fn mismatched_legacy_projection_and_unknown_extensions_fail_closed() {
        let envelope = envelope(
            ScopeContext::organization(InstallationId::new(), OrganizationId::new())
                .expect("scope"),
        );
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
