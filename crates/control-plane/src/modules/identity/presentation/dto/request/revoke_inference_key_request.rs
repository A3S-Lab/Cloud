use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeInferenceKeyRequest {
    pub expected_aggregate_version: u64,
}
