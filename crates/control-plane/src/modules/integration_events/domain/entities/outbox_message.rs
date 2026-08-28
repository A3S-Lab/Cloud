use crate::modules::shared_kernel::domain::ScopeContext;
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboxMessage {
    pub event_id: Uuid,
    pub event_key: String,
    pub schema_version: u32,
    pub scope: ScopeContext,
    pub aggregate_id: Uuid,
    pub aggregate_version: u64,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub delivery_attempts: u32,
}

impl OutboxMessage {
    pub const fn organization_id(&self) -> Option<Uuid> {
        match self.scope.organization_id() {
            Some(value) => Some(value.as_uuid()),
            None => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.scope.validate()?;
        if self.event_id.is_nil()
            || self.event_key.is_empty()
            || self.schema_version == 0
            || self.aggregate_id.is_nil()
            || self.aggregate_version == 0
            || self.correlation_id.is_nil()
            || self.causation_id.is_some_and(|value| value.is_nil())
            || !self.payload.is_object()
        {
            return Err("Outbox message is invalid".into());
        }
        Ok(())
    }

    /// Restores the owner-domain envelope from one committed Outbox fact.
    ///
    /// The committed scope retains its database-resolved Installation identity;
    /// the owner-domain reference intentionally projects only the lineage a
    /// domain fact is allowed to carry before persistence resolves ownership.
    pub fn domain_event(&self) -> Result<DomainEventEnvelope, String> {
        self.validate()?;
        let event = DomainEventEnvelope {
            event_id: self.event_id,
            event_key: self.event_key.clone(),
            schema_version: self.schema_version,
            scope: self.scope.reference(),
            aggregate_id: self.aggregate_id,
            aggregate_version: self.aggregate_version,
            occurred_at: self.occurred_at,
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
            payload: self.payload.clone(),
        };
        event.validate()?;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::InstallationId;

    fn message(scope: ScopeContext) -> OutboxMessage {
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "identity.platform-role.changed".into(),
            schema_version: 1,
            scope,
            aggregate_id: Uuid::now_v7(),
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({"changed": true}),
            delivery_attempts: 1,
        }
    }

    #[test]
    fn committed_outbox_message_accepts_installation_scope_without_a_fake_organization() {
        let value =
            message(ScopeContext::installation(InstallationId::new()).expect("Installation scope"));
        assert_eq!(value.organization_id(), None);
        assert_eq!(value.validate(), Ok(()));
    }

    #[test]
    fn committed_outbox_message_rejects_forged_scope_identity() {
        let value = message(ScopeContext::Installation {
            installation_id: InstallationId::from_uuid(Uuid::nil()),
        });
        assert!(value.validate().is_err());
    }

    #[test]
    fn committed_message_has_one_checked_adapter_to_its_owner_domain_envelope() {
        let installation_id = InstallationId::new();
        let organization_id = crate::modules::shared_kernel::domain::OrganizationId::new();
        let value = message(
            ScopeContext::organization(installation_id, organization_id)
                .expect("Organization scope"),
        );
        let event = value.domain_event().expect("domain event");
        assert_eq!(
            event.scope,
            a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: organization_id.as_uuid(),
            }
        );
        assert_eq!(event.event_id, value.event_id);
        assert_eq!(event.payload, value.payload);
    }
}
