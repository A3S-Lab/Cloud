use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeMcpCredentialRequest {
    pub expected_aggregate_version: u64,
}
