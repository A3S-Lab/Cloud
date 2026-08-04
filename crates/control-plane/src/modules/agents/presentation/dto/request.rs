use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartAgentExecutionRequest {
    pub agent_asset_id: Uuid,
    pub agent_asset_release_id: Uuid,
    #[serde(default)]
    pub input: serde_json::Value,
}
