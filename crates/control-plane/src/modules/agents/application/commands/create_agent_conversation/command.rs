use crate::modules::agents::domain::AgentConversation;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateAgentConversation {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateAgentConversation {
    type Output = ApplicationResult<CreateAgentConversationResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateAgentConversationResult {
    pub conversation: AgentConversation,
    pub replayed: bool,
}
