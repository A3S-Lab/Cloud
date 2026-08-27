use crate::modules::agents::domain::{
    validate_agent_approval_reason, AgentApprovalCheckpoint, AgentApprovalCheckpointStatus,
    AgentConversation, AgentExecution, AgentExecutionStatus, NewAgentApprovalCheckpoint,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentApprovalCheckpointId, AgentApprovalDecisionId, AgentExecutionId,
    AuthorizationDecisionRef, IdempotencyRequest, NodeCommandId, OrganizationId, PrincipalId,
    RepositoryError, Sha256Digest,
};
use a3s_cloud_contracts::{
    AgentProviderApprovalOutcomeV1, AgentProviderCommandV1, AgentProviderEventPageV1,
    AgentProviderRunStateV1, AgentProviderSemanticEventV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub fn project_agent_approval_checkpoint(
    conversation: &AgentConversation,
    execution: &AgentExecution,
    page: &AgentProviderEventPageV1,
    requested_at: DateTime<Utc>,
) -> Result<Option<AgentApprovalCheckpoint>, String> {
    let Some(record) = page.events.iter().find(|record| {
        matches!(
            &record.event,
            AgentProviderSemanticEventV1::ToolRequest { tool, .. } if tool.approval_required
        )
    }) else {
        return Ok(None);
    };
    let AgentProviderSemanticEventV1::ToolRequest {
        call_id,
        tool,
        request,
    } = &record.event
    else {
        return Err("Agent approval projection selected another provider event".into());
    };
    let binding = execution
        .code
        .as_ref()
        .ok_or_else(|| "Agent approval checkpoint has no provider binding".to_owned())?;
    let invocation_profile = binding.require_invocation_profile()?;
    let invocation_profile_digest = Sha256Digest::parse(invocation_profile.digest()?)?;
    let run_identity_digest = Sha256Digest::parse(page.identity.digest()?)?;
    if execution.organization_id != conversation.organization_id
        || execution.conversation_id != conversation.id
        || execution.status != AgentExecutionStatus::AwaitingApproval
        || page.state != AgentProviderRunStateV1::AwaitingApproval
        || binding.observed_state() != AgentProviderRunStateV1::AwaitingApproval
        || page.identity.invocation_profile_digest.as_deref()
            != Some(invocation_profile_digest.as_str())
        || !invocation_profile.tools.contains(tool)
    {
        return Err("Agent approval checkpoint changed its execution or invocation binding".into());
    }
    let id = AgentApprovalCheckpoint::deterministic_id(
        execution.id,
        &run_identity_digest,
        record.sequence,
        call_id,
        &request.digest,
    );
    AgentApprovalCheckpoint::create(NewAgentApprovalCheckpoint {
        organization_id: execution.organization_id,
        project_id: conversation.project_id,
        environment_id: conversation.environment_id,
        conversation_id: conversation.id,
        execution_id: execution.id,
        id,
        provider_run_identity_digest: run_identity_digest,
        invocation_profile_digest,
        source_event_sequence: record.sequence,
        call_id: call_id.clone(),
        tool: tool.clone(),
        request: request.clone(),
        requested_at,
    })
    .map(Some)
}

#[derive(Debug, Clone)]
pub struct DecideAgentApprovalCheckpointWrite {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentApprovalCheckpointId,
    pub expected_version: u64,
    pub decision_id: AgentApprovalDecisionId,
    pub outcome: AgentProviderApprovalOutcomeV1,
    pub decided_by: PrincipalId,
    pub authorization_decision: AuthorizationDecisionRef,
    pub reason: Option<String>,
    pub decided_at: DateTime<Utc>,
    pub request_id: uuid::Uuid,
    pub idempotency: IdempotencyRequest,
}

impl DecideAgentApprovalCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.authorization_decision.validate()?;
        self.idempotency.validate()?;
        validate_agent_approval_reason(self.reason.as_deref())?;
        if self.organization_id.as_uuid().is_nil()
            || self.checkpoint_id.as_uuid().is_nil()
            || self.decision_id.as_uuid().is_nil()
            || self.decided_by.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.expected_version == 0
            || !matches!(
                self.outcome,
                AgentProviderApprovalOutcomeV1::Approved | AgentProviderApprovalOutcomeV1::Denied
            )
            || self.decided_at != canonical_timestamp(self.decided_at)
        {
            return Err("Agent approval decision write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ExpireAgentApprovalCheckpointWrite {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentApprovalCheckpointId,
    pub expected_version: u64,
    pub decision_id: AgentApprovalDecisionId,
    pub expired_at: DateTime<Utc>,
}

impl ExpireAgentApprovalCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.checkpoint_id.as_uuid().is_nil()
            || self.decision_id.as_uuid().is_nil()
            || self.expected_version == 0
            || self.expired_at != canonical_timestamp(self.expired_at)
        {
            return Err("Agent approval expiry write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ResumeAgentApprovalCheckpointWrite {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentApprovalCheckpointId,
    pub expected_version: u64,
    pub command_id: NodeCommandId,
    pub command: AgentProviderCommandV1,
    pub resumed_at: DateTime<Utc>,
}

impl ResumeAgentApprovalCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.command.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.checkpoint_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.expected_version == 0
            || !matches!(&self.command, AgentProviderCommandV1::Resume { .. })
            || self.resumed_at != canonical_timestamp(self.resumed_at)
        {
            return Err("Agent approval resume write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CancelActiveAgentApprovalCheckpointWrite {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub cancelled_at: DateTime<Utc>,
}

impl CancelActiveAgentApprovalCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.cancelled_at != canonical_timestamp(self.cancelled_at)
        {
            return Err("Agent approval cancellation write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentApprovalCheckpointWrite {
    pub checkpoint: AgentApprovalCheckpoint,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentApprovalCheckpointWriteReference {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentApprovalCheckpointId,
}

#[async_trait]
pub trait IAgentApprovalCheckpointRepository: Send + Sync {
    async fn replay_checkpoint_decision(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError>;

    async fn decide_checkpoint(
        &self,
        write: DecideAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError>;

    async fn expire_checkpoint(
        &self,
        write: ExpireAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError>;

    async fn mark_checkpoint_resumed(
        &self,
        write: ResumeAgentApprovalCheckpointWrite,
    ) -> Result<AgentApprovalCheckpointWrite, RepositoryError>;

    async fn cancel_active_checkpoint(
        &self,
        write: CancelActiveAgentApprovalCheckpointWrite,
    ) -> Result<Option<AgentApprovalCheckpointWrite>, RepositoryError>;

    async fn find_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentApprovalCheckpointId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError>;

    async fn find_active_checkpoint(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentApprovalCheckpoint>, RepositoryError>;

    async fn list_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        status: Option<AgentApprovalCheckpointStatus>,
        limit: usize,
    ) -> Result<Vec<AgentApprovalCheckpoint>, RepositoryError>;
}
