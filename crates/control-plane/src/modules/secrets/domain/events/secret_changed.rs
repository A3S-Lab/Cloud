use crate::modules::secrets::domain::{Secret, SecretVersion};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, SecretId};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretChanged {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub secret_id: SecretId,
    pub name: String,
    pub state: String,
    pub version: u64,
    pub version_state: String,
}

impl SecretChanged {
    pub fn created(
        secret: &Secret,
        version: &SecretVersion,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("secret.secret.created", secret, version, correlation_id)
    }

    pub fn rotated(
        secret: &Secret,
        version: &SecretVersion,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("secret.version.created", secret, version, correlation_id)
    }

    pub fn version_revoked(
        secret: &Secret,
        version: &SecretVersion,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("secret.version.revoked", secret, version, correlation_id)
    }

    fn envelope(
        event_key: &str,
        secret: &Secret,
        version: &SecretVersion,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: secret.organization_id.as_uuid(),
            aggregate_id: secret.id.as_uuid(),
            aggregate_version: secret.aggregate_version,
            occurred_at: secret.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: secret.organization_id,
                project_id: secret.project_id,
                environment_id: secret.environment_id,
                secret_id: secret.id,
                name: secret.name.as_str().to_owned(),
                state: secret.state.as_str().to_owned(),
                version: version.version,
                version_state: version.state.as_str().to_owned(),
            })?,
        })
    }
}
