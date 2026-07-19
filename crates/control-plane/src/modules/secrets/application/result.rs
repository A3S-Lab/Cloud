use crate::modules::secrets::domain::{Secret, SecretVersion, SecretVersionState, SecretWrite};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretVersionResult {
    pub version: u64,
    pub state: SecretVersionState,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl From<&SecretVersion> for SecretVersionResult {
    fn from(version: &SecretVersion) -> Self {
        Self {
            version: version.version,
            state: version.state,
            aggregate_version: version.aggregate_version,
            created_at: version.created_at,
            revoked_at: version.revoked_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretMutationResult {
    pub secret: Secret,
    pub version: SecretVersionResult,
    pub replayed: bool,
}

impl From<SecretWrite> for SecretMutationResult {
    fn from(write: SecretWrite) -> Self {
        Self {
            secret: write.secret,
            version: SecretVersionResult::from(&write.version),
            replayed: write.replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SecretDetails {
    pub secret: Secret,
    pub versions: Vec<SecretVersionResult>,
}
