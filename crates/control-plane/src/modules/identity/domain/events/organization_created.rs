use crate::modules::identity::domain::entities::Organization;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationCreated {
    pub organization_id: OrganizationId,
    pub name: String,
}

impl OrganizationCreated {
    pub fn envelope(
        organization: &Organization,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            organization_id: organization.id,
            name: organization.name.as_str().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.organization.created".into(),
            schema_version: 1,
            organization_id: organization.id.as_uuid(),
            aggregate_id: organization.id.as_uuid(),
            aggregate_version: organization.aggregate_version,
            occurred_at: organization.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
