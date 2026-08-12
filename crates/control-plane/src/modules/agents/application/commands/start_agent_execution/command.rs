use crate::modules::agents::domain::{AgentConversation, AgentExecution};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AssetId, AssetReleaseId, OrganizationId,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StartAgentExecution {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub resource_access: ResourceAccessEvaluator,
    pub agent_asset_id: AssetId,
    pub agent_asset_release_id: AssetReleaseId,
    pub input: serde_json::Value,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for StartAgentExecution {
    type Output = ApplicationResult<StartAgentExecutionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartAgentExecutionResult {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
    pub replayed: bool,
}
