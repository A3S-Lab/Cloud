use crate::modules::agents::domain::{AgentConversation, AgentExecution};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, OrganizationId,
};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ForkAgentExecution {
    pub organization_id: OrganizationId,
    pub parent_execution_id: AgentExecutionId,
    pub checkpoint_id: AgentExecutionCheckpointId,
    pub resource_access: ResourceAccessEvaluator,
    pub input: serde_json::Value,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for ForkAgentExecution {
    type Output = ApplicationResult<ForkAgentExecutionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ForkAgentExecutionResult {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
    pub replayed: bool,
}
