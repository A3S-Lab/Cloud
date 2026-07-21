use crate::modules::sources::domain::entities::GithubConnection;
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConnectionReconciled {
    pub organization_id: Uuid,
    pub source_connection_id: Uuid,
    pub installation_id: u64,
    pub account_id: u64,
    pub account_login: String,
    pub status: String,
}

impl GithubConnectionReconciled {
    pub fn envelope(
        connection: &GithubConnection,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "source.github-connection.reconciled".into(),
            schema_version: 1,
            organization_id: connection.organization_id.as_uuid(),
            aggregate_id: connection.id.as_uuid(),
            aggregate_version: connection.aggregate_version,
            occurred_at: connection.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: connection.organization_id.as_uuid(),
                source_connection_id: connection.id.as_uuid(),
                installation_id: connection.installation_id.as_u64(),
                account_id: connection.account_id.as_u64(),
                account_login: connection.account_login.as_str().into(),
                status: connection.status.as_str().into(),
            })?,
        })
    }
}
