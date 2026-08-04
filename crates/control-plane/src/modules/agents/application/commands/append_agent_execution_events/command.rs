use crate::modules::agents::domain::{AgentExecutionEventDraft, AgentExecutionEventsWrite};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionId, OrganizationId,
};
use a3s_boot::Command;

#[derive(Debug, Clone)]
pub struct AppendAgentExecutionEvents {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub events: Vec<AgentExecutionEventDraft>,
    pub idempotency_key: String,
}

impl Command for AppendAgentExecutionEvents {
    type Output = ApplicationResult<AgentExecutionEventsWrite>;
}
