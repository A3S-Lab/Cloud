use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_runtime::contract::RuntimeObservation;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Agents-owned projection of one durable Node command used by AgentExecution Flow.
///
/// Fleet remains the command-store authority. Agents only sees the consumer-facing
/// fields required to enqueue, reload, and validate Agent provider commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionNodeCommandProjection {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Agents-owned enqueue request for one Agent provider Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionNodeCommandEnqueueRequest {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Agents-owned enqueue dispatch fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionNodeCommandDispatch {
    pub command: AgentExecutionNodeCommandProjection,
    pub replayed: bool,
}

/// Agents-owned acknowledgement projection for one Agent provider Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionNodeCommandAcknowledgement {
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

/// Agents-owned Runtime observation projection for Code Harness binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionRuntimeObservationProjection {
    pub node_id: NodeId,
    pub command_id: Option<NodeCommandId>,
    pub received_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

/// Agents-owned port for Node command enqueue/reload/ack and Runtime observation
/// reads required by AgentExecution Flow. Fleet remains the sole Node-command
/// authority behind the ACA.
#[async_trait]
pub trait IAgentExecutionNodeCommandPort: Send + Sync {
    async fn enqueue_command(
        &self,
        request: AgentExecutionNodeCommandEnqueueRequest,
    ) -> Result<AgentExecutionNodeCommandDispatch, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<AgentExecutionNodeCommandProjection>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<AgentExecutionNodeCommandAcknowledgement>, RepositoryError>;

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<AgentExecutionRuntimeObservationProjection>, RepositoryError>;
}
