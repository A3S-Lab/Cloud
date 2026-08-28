use crate::modules::identity::domain::entities::IdentityPrincipal;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalCreated {
    pub principal_id: Uuid,
    pub kind: String,
    pub name: String,
}

impl PrincipalCreated {
    pub fn envelope(
        organization_id: OrganizationId,
        principal: &IdentityPrincipal,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            principal_id: principal.id.as_uuid(),
            kind: principal.kind.as_str().to_owned(),
            name: principal.name.as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.principal.created".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: organization_id.as_uuid(),
            },
            aggregate_id: principal.id.as_uuid(),
            aggregate_version: principal.aggregate_version,
            occurred_at: principal.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
