use crate::modules::sources::domain::entities::GithubConnection;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConnectionCreated {
    pub organization_id: Uuid,
    pub source_connection_id: Uuid,
    pub installation_id: u64,
    pub account_id: u64,
    pub account_kind: String,
    pub verified_by_user_id: u64,
}

impl GithubConnectionCreated {
    pub fn envelope(
        connection: &GithubConnection,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "source.github-connection.created".into(),
            schema_version: 1,
            organization_id: connection.organization_id.as_uuid(),
            aggregate_id: connection.id.as_uuid(),
            aggregate_version: connection.aggregate_version,
            occurred_at: connection.connected_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: connection.organization_id.as_uuid(),
                source_connection_id: connection.id.as_uuid(),
                installation_id: connection.installation_id.as_u64(),
                account_id: connection.account_id.as_u64(),
                account_kind: connection.account_kind.as_str().into(),
                verified_by_user_id: connection.verified_by_user_id.as_u64(),
            })?,
        })
    }
}
