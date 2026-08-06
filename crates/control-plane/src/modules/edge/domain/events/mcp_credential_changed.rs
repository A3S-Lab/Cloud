use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCredentialChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
    pub generation: u64,
    pub state: String,
    pub expires_at: DateTime<Utc>,
}

impl McpCredentialChanged {
    pub fn created(
        credential: &McpCredential,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("edge.mcp-credential.created", credential, correlation_id)
    }

    pub fn rotated(
        credential: &McpCredential,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("edge.mcp-credential.rotated", credential, correlation_id)
    }

    pub fn revoked(
        credential: &McpCredential,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("edge.mcp-credential.revoked", credential, correlation_id)
    }

    fn envelope(
        event_key: &str,
        credential: &McpCredential,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: credential.organization_id.as_uuid(),
            aggregate_id: credential.id.as_uuid(),
            aggregate_version: credential.aggregate_version(),
            occurred_at: credential.updated_at(),
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: credential.organization_id,
                project_id: credential.project_id,
                environment_id: credential.environment_id,
                credential_id: credential.id,
                generation: credential.generation(),
                state: if credential.revoked_at().is_some() {
                    "revoked".into()
                } else {
                    "active".into()
                },
                expires_at: credential.expires_at(),
            })?,
        })
    }
}
