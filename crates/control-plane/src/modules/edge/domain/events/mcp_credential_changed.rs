use crate::modules::edge::domain::McpCredential;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpCredentialChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub credential_id: McpCredentialId,
    pub generation: u64,
    pub expires_at: DateTime<Utc>,
    pub revoked: bool,
}

impl McpCredentialChanged {
    pub fn envelope(
        credential: &McpCredential,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let event_key = if credential.revoked_at().is_some() {
            "edge.mcp-credential.revoked"
        } else if credential.generation() == 1 {
            "edge.mcp-credential.issued"
        } else {
            "edge.mcp-credential.rotated"
        };
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
                expires_at: credential.expires_at(),
                revoked: credential.revoked_at().is_some(),
            })?,
        })
    }
}
