use crate::modules::agents::domain::{AgentConversation, AgentExecution};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CancelAgentExecution {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelAgentExecution {
    type Output = ApplicationResult<CancelAgentExecutionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelAgentExecutionResult {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
    pub replayed: bool,
}
