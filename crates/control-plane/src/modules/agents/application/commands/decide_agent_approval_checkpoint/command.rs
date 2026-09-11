use crate::modules::agents::application::AgentAccess;
use crate::modules::agents::domain::AgentApprovalCheckpoint;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentExecutionId, ApiTokenId, OrganizationId, PrincipalId,
};
use a3s_boot::Command;
use a3s_cloud_contracts::AgentProviderApprovalOutcomeV1;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DecideAgentApprovalCheckpoint {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentApprovalCheckpointId,
    pub expected_version: u64,
    pub outcome: AgentProviderApprovalOutcomeV1,
    pub reason: Option<String>,
    pub access: AgentAccess,
    pub actor_principal_id: PrincipalId,
    pub credential_id: ApiTokenId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for DecideAgentApprovalCheckpoint {
    type Output = ApplicationResult<DecideAgentApprovalCheckpointResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideAgentApprovalCheckpointResult {
    pub checkpoint: AgentApprovalCheckpoint,
    pub replayed: bool,
}
