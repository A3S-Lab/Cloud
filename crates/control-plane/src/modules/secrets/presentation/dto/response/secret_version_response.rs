use crate::modules::secrets::application::SecretVersionResult;
use crate::modules::secrets::domain::SecretVersionState;
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretVersionResponse {
    pub version: u64,
    pub state: SecretVersionState,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<SecretVersionResult> for SecretVersionResponse {
    fn from(version: SecretVersionResult) -> Self {
        Self {
            version: version.version,
            state: version.state,
            aggregate_version: version.aggregate_version,
            created_at: version.created_at,
            revoked_at: version.revoked_at,
        }
    }
}
