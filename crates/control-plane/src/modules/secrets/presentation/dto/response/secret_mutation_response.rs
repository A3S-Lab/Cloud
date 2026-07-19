use super::SecretVersionResponse;
use crate::modules::secrets::application::SecretMutationResult;
use crate::modules::secrets::domain::SecretState;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMutationResponse {
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
    pub version: SecretVersionResponse,
    pub replayed: bool,
}

impl From<SecretMutationResult> for SecretMutationResponse {
    fn from(result: SecretMutationResult) -> Self {
        Self {
            id: result.secret.id.as_uuid(),
            organization_id: result.secret.organization_id.as_uuid(),
            project_id: result.secret.project_id.as_uuid(),
            environment_id: result.secret.environment_id.as_uuid(),
            name: result.secret.name.as_str().to_owned(),
            state: result.secret.state,
            current_version: result.secret.current_version,
            aggregate_version: result.secret.aggregate_version,
            created_at: result.secret.created_at,
            updated_at: result.secret.updated_at,
            revoked_at: result.secret.revoked_at,
            version: result.version.into(),
            replayed: result.replayed,
        }
    }
}
