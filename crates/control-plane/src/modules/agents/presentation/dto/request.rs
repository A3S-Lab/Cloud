use serde::Deserialize;
use uuid::Uuid;

use crate::modules::agents::domain::NATIVE_CODE_AGENT_PROVIDER_KIND;

fn default_agent_provider_kind() -> String {
    NATIVE_CODE_AGENT_PROVIDER_KIND.into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartAgentExecutionRequest {
    pub agent_asset_id: Uuid,
    pub agent_asset_release_id: Uuid,
    #[serde(default = "default_agent_provider_kind")]
    pub provider_kind: String,
    #[serde(default)]
    pub input: serde_json::Value,
}
