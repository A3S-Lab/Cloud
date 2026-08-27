use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionEvent, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, IdempotencyRequest, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CommitAgentExecutionCheckpointWrite {
    pub checkpoint: AgentExecutionCheckpoint,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl CommitAgentExecutionCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.checkpoint.validate()?;
        let checkpoint = &self.checkpoint;
        if self.event.event_key != "agent.execution-checkpoint.committed"
            || self.event.schema_version != 1
            || self.event.organization_id != checkpoint.organization_id.as_uuid()
            || self.event.aggregate_id != checkpoint.id.as_uuid()
            || self.event.aggregate_version != checkpoint.aggregate_version
            || self.event.occurred_at != checkpoint.captured_at
            || self.event.event_id.is_nil()
            || self.event.correlation_id.is_nil()
        {
            return Err("Agent checkpoint event does not match its projection".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointWrite {
    pub checkpoint: AgentExecutionCheckpoint,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct ForkAgentExecutionWrite {
    pub execution: crate::modules::agents::domain::AgentExecution,
    pub initial_event: AgentExecutionEventDraft,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl ForkAgentExecutionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.execution.validate()?;
        self.initial_event.content.validate()?;
        let lineage = self
            .execution
            .lineage
            .as_ref()
            .ok_or_else(|| "forked Agent execution has no lineage".to_owned())?;
        if self.initial_event.kind != AgentExecutionEventKind::ExecutionRequested
            || self.initial_event.occurred_at != self.execution.requested_at
            || self.execution.aggregate_version != 1
            || lineage.parent_execution_id == self.execution.id
            || self.event.event_key != "agent.execution.forked"
            || self.event.schema_version != 1
            || self.event.organization_id != self.execution.organization_id.as_uuid()
            || self.event.aggregate_id != self.execution.id.as_uuid()
            || self.event.aggregate_version != self.execution.aggregate_version
            || self.event.occurred_at != self.execution.requested_at
            || self.event.event_id.is_nil()
            || self.event.correlation_id.is_nil()
        {
            return Err("Agent execution fork event does not match its aggregate".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionCheckpointWriteReference {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentExecutionCheckpointId,
}

#[async_trait]
pub trait IAgentExecutionCheckpointRepository: Send + Sync {
    async fn commit_execution_checkpoint(
        &self,
        write: CommitAgentExecutionCheckpointWrite,
    ) -> Result<AgentExecutionCheckpointWrite, RepositoryError>;

    async fn fork_execution(
        &self,
        write: ForkAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError>;

    async fn replay_execution_checkpoint(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError>;

    async fn find_execution_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentExecutionCheckpointId,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError>;

    async fn list_execution_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        limit: usize,
    ) -> Result<Vec<AgentExecutionCheckpoint>, RepositoryError>;

    async fn list_execution_trajectory_events(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        after_sequence: Option<u64>,
        through_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError>;
}
