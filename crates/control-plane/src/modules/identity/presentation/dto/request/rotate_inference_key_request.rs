use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RotateInferenceKeyRequest {
    pub expected_aggregate_version: u64,
    pub expires_at: DateTime<Utc>,
}
