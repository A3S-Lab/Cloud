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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureAgentExecutionCheckpointRequest {
    pub through_event_sequence: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForkAgentExecutionRequest {
    #[serde(default)]
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApprovalDecisionRequestOutcome {
    Approved,
    Denied,
}

impl From<AgentApprovalDecisionRequestOutcome>
    for a3s_cloud_contracts::AgentProviderApprovalOutcomeV1
{
    fn from(value: AgentApprovalDecisionRequestOutcome) -> Self {
        match value {
            AgentApprovalDecisionRequestOutcome::Approved => Self::Approved,
            AgentApprovalDecisionRequestOutcome::Denied => Self::Denied,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentApprovalDecisionRequest {
    pub outcome: AgentApprovalDecisionRequestOutcome,
    pub reason: Option<String>,
}
