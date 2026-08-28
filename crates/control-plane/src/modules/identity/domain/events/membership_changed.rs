use crate::modules::identity::domain::entities::Membership;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipChanged {
    pub membership_id: Uuid,
    pub principal_id: Uuid,
    pub role: String,
}

impl MembershipChanged {
    pub fn created(
        membership: &Membership,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("identity.membership.created", membership, correlation_id)
    }

    pub fn role_changed(
        membership: &Membership,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope(
            "identity.membership.role-changed",
            membership,
            correlation_id,
        )
    }

    pub fn revoked(
        membership: &Membership,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("identity.membership.revoked", membership, correlation_id)
    }

    fn envelope(
        event_key: &str,
        membership: &Membership,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            membership_id: membership.id.as_uuid(),
            principal_id: membership.principal_id.as_uuid(),
            role: membership.role.as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: membership.organization_id.as_uuid(),
            },
            aggregate_id: membership.id.as_uuid(),
            aggregate_version: membership.aggregate_version,
            occurred_at: membership.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
