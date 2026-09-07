use serde::{Deserialize, Serialize};

use super::PluginRegistryResponse;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRegistryMutationResponse {
    pub registry: PluginRegistryResponse,
    pub replayed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnrollPluginRegistryRequest {
    pub name: String,
    pub endpoint: String,
    pub bootstrap_root_base64: String,
}
