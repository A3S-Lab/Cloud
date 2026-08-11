use crate::modules::agents::domain::{
    AgentCodeRunBinding, AgentConversation, AgentExecution, AgentExecutionChangeSet,
    AgentExecutionEvent, AgentExecutionEventDraft, AgentExecutionEventKind, AgentExecutionStatus,
    MAX_AGENT_EVENTS_PER_APPEND,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, AgentConversationId, AgentExecutionId, EnvironmentId, IdempotencyRequest,
    NodeId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_cloud_contracts::{
    DomainEventEnvelope, NodeCodeAgentEventBatchV1, NodeCodeAgentEventReceiptV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct CreateAgentConversationWrite {
    pub conversation: AgentConversation,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl CreateAgentConversationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.conversation.validate()?;
        let conversation = &self.conversation;
        let event = &self.event;
        if conversation.aggregate_version != 1
            || conversation.last_event_sequence != 0
            || event.event_key != "agent.conversation.created"
            || event.schema_version != 1
            || event.organization_id != conversation.organization_id.as_uuid()
            || event.aggregate_id != conversation.id.as_uuid()
            || event.aggregate_version != conversation.aggregate_version
            || event.occurred_at != conversation.created_at
            || event.event_id.is_nil()
            || event.correlation_id.is_nil()
        {
            return Err("Agent conversation event does not match its aggregate".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConversationWrite {
    pub conversation: AgentConversation,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct StartAgentExecutionWrite {
    pub execution: AgentExecution,
    pub initial_event: AgentExecutionEventDraft,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl StartAgentExecutionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.execution.validate()?;
        self.initial_event.content.validate()?;
        let execution = &self.execution;
        let event = &self.event;
        if self.initial_event.kind != AgentExecutionEventKind::ExecutionRequested
            || self.initial_event.occurred_at != execution.requested_at
            || execution.aggregate_version != 1
            || event.event_key != "agent.execution.started"
            || event.schema_version != 1
            || event.organization_id != execution.organization_id.as_uuid()
            || event.aggregate_id != execution.id.as_uuid()
            || event.aggregate_version != execution.aggregate_version
            || event.occurred_at != execution.requested_at
            || event.event_id.is_nil()
            || event.correlation_id.is_nil()
        {
            return Err("Agent execution event does not match its aggregate".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionWrite {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct RequestAgentExecutionCancellationWrite {
    pub execution: AgentExecution,
    pub expected_version: u64,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl RequestAgentExecutionCancellationWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.execution.validate()?;
        let execution = &self.execution;
        let event = &self.event;
        if self.expected_version == 0
            || self
                .expected_version
                .checked_add(1)
                .is_none_or(|version| execution.aggregate_version != version)
            || execution.status != AgentExecutionStatus::Cancelling
            || execution.cancellation_requested_at != Some(execution.updated_at)
            || event.event_key != "agent.execution.cancellation-requested"
            || event.schema_version != 1
            || event.organization_id != execution.organization_id.as_uuid()
            || event.aggregate_id != execution.id.as_uuid()
            || event.aggregate_version != execution.aggregate_version
            || event.occurred_at != execution.updated_at
            || event.event_id.is_nil()
            || event.correlation_id.is_nil()
        {
            return Err("Agent execution cancellation event does not match its aggregate".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AppendAgentExecutionEventsWrite {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub events: Vec<AgentExecutionEventDraft>,
    pub idempotency: IdempotencyRequest,
}

impl AppendAgentExecutionEventsWrite {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.conversation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.events.is_empty()
            || self.events.len() > MAX_AGENT_EVENTS_PER_APPEND
        {
            return Err("Agent event append is invalid".into());
        }
        let mut previous = None;
        for event in &self.events {
            event.content.validate()?;
            if previous.is_some_and(|occurred_at| event.occurred_at < occurred_at) {
                return Err("Agent event batch timestamps must be monotonic".into());
            }
            previous = Some(event.occurred_at);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionEventsWrite {
    pub conversation: AgentConversation,
    pub execution: AgentExecution,
    pub events: Vec<AgentExecutionEvent>,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct BindAgentCodeRunWrite {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub binding: AgentCodeRunBinding,
}

impl BindAgentCodeRunWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.organization_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || !self.binding.is_initial()
        {
            return Err("Agent Code run bind write is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCodeRunWrite {
    pub execution: AgentExecution,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct AcceptAgentCodeEventBatchWrite {
    pub organization_id: OrganizationId,
    pub authenticated_node_id: NodeId,
    pub batch: NodeCodeAgentEventBatchV1,
    pub accepted_at: DateTime<Utc>,
    pub idempotency: IdempotencyRequest,
}

impl AcceptAgentCodeEventBatchWrite {
    pub fn new(
        organization_id: OrganizationId,
        authenticated_node_id: NodeId,
        batch: NodeCodeAgentEventBatchV1,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let encoded = serde_json::to_vec(&batch)
            .map_err(|error| format!("could not encode Code Agent event batch: {error}"))?;
        let idempotency = IdempotencyRequest::new(
            Self::idempotency_scope(organization_id, authenticated_node_id),
            batch.batch_id.to_string(),
            &encoded,
        )?;
        let write = Self {
            organization_id,
            authenticated_node_id,
            batch,
            accepted_at: canonical_timestamp(accepted_at),
            idempotency,
        };
        write.validate()?;
        Ok(write)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.batch.validate()?;
        self.accepted_at_ms()?;
        let encoded = serde_json::to_vec(&self.batch)
            .map_err(|error| format!("could not encode Code Agent event batch: {error}"))?;
        let expected_idempotency = IdempotencyRequest::new(
            Self::idempotency_scope(self.organization_id, self.authenticated_node_id),
            self.batch.batch_id.to_string(),
            &encoded,
        )?;
        if self.organization_id.as_uuid().is_nil()
            || self.authenticated_node_id.as_uuid() != self.batch.node_id
            || self.batch.binding.execution_id.is_nil()
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.idempotency != expected_idempotency
        {
            return Err("Code Agent event batch write is invalid".into());
        }
        Ok(())
    }

    pub fn accepted_at_ms(&self) -> Result<u64, String> {
        u64::try_from(self.accepted_at.timestamp_millis())
            .map_err(|_| "Code Agent event acceptance time is invalid".to_owned())
    }

    fn idempotency_scope(organization_id: OrganizationId, node_id: NodeId) -> String {
        format!("organizations/{organization_id}/nodes/{node_id}/code-agent-event-batches")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConversationWriteReference {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionWriteReference {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionEventsWriteReference {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub execution_id: AgentExecutionId,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[async_trait]
pub trait IAgentRepository: Send + Sync {
    async fn create_conversation(
        &self,
        write: CreateAgentConversationWrite,
    ) -> Result<AgentConversationWrite, RepositoryError>;

    async fn start_execution(
        &self,
        write: StartAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError>;

    async fn request_cancellation(
        &self,
        write: RequestAgentExecutionCancellationWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError>;

    async fn append_events(
        &self,
        write: AppendAgentExecutionEventsWrite,
    ) -> Result<AgentExecutionEventsWrite, RepositoryError>;

    async fn bind_code_run(
        &self,
        write: BindAgentCodeRunWrite,
    ) -> Result<AgentCodeRunWrite, RepositoryError>;

    async fn accept_code_event_batch(
        &self,
        write: AcceptAgentCodeEventBatchWrite,
    ) -> Result<NodeCodeAgentEventReceiptV1, RepositoryError>;

    async fn replay_conversation(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentConversation>, RepositoryError>;

    async fn replay_execution(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecution>, RepositoryError>;

    async fn find_conversation(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
    ) -> Result<Option<AgentConversation>, RepositoryError>;

    async fn list_conversations(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        limit: usize,
    ) -> Result<Vec<AgentConversation>, RepositoryError>;

    async fn find_execution(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecution>, RepositoryError>;

    async fn find_execution_change_set(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionChangeSet>, RepositoryError>;

    async fn pending_operation_starts(
        &self,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError>;

    async fn find_execution_request(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
    ) -> Result<Option<AgentExecutionEvent>, RepositoryError>;

    async fn list_executions(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        limit: usize,
    ) -> Result<Vec<AgentExecution>, RepositoryError>;

    async fn list_events(
        &self,
        organization_id: OrganizationId,
        conversation_id: AgentConversationId,
        after_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError>;
}
