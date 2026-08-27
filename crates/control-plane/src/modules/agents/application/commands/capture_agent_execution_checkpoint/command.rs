use crate::modules::agents::domain::AgentExecutionCheckpoint;
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::Command;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CaptureAgentExecutionCheckpoint {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
    pub through_event_sequence: Option<u64>,
    pub idempotency_key: String,
    pub request_id: Uuid,
}

impl Command for CaptureAgentExecutionCheckpoint {
    type Output = ApplicationResult<CaptureAgentExecutionCheckpointResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureAgentExecutionCheckpointResult {
    pub checkpoint: AgentExecutionCheckpoint,
    pub replayed: bool,
}
