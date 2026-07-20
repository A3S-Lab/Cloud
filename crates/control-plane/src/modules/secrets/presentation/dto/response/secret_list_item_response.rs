use crate::modules::secrets::domain::{Secret, SecretState};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretListItemResponse {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub name: String,
    pub state: SecretState,
    pub current_version: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<Secret> for SecretListItemResponse {
    fn from(secret: Secret) -> Self {
        Self {
            id: secret.id.as_uuid(),
            organization_id: secret.organization_id.as_uuid(),
            project_id: secret.project_id.as_uuid(),
            environment_id: secret.environment_id.as_uuid(),
            name: secret.name.as_str().to_owned(),
            state: secret.state,
            current_version: secret.current_version,
            aggregate_version: secret.aggregate_version,
            created_at: secret.created_at,
            updated_at: secret.updated_at,
            revoked_at: secret.revoked_at,
        }
    }
}
