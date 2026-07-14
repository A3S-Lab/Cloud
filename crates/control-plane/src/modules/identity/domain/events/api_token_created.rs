use crate::modules::identity::domain::entities::ApiToken;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{ApiTokenId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiTokenCreated {
    pub token_id: ApiTokenId,
    pub organization_id: OrganizationId,
    pub name: String,
    pub scopes: Vec<ApiTokenScope>,
}

impl ApiTokenCreated {
    pub fn envelope(
        token: &ApiToken,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            token_id: token.id,
            organization_id: token.organization_id,
            name: token.name.as_str().to_owned(),
            scopes: token.scopes.iter().cloned().collect(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "identity.token.created".into(),
            schema_version: 1,
            organization_id: token.organization_id.as_uuid(),
            aggregate_id: token.id.as_uuid(),
            aggregate_version: token.aggregate_version,
            occurred_at: token.created_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
