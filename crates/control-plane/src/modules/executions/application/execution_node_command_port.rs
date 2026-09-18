use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use a3s_runtime::contract::RuntimeObservation;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Executions-owned projection of one durable Node command used by Execution Flow.
///
/// Fleet remains the command-store authority. Executions only sees the
/// consumer-facing fields required to enqueue, reload, and validate Runtime
/// apply/remove commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionNodeCommandProjection {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Executions-owned enqueue request for one Runtime Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionNodeCommandEnqueueRequest {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Executions-owned enqueue dispatch fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionNodeCommandDispatch {
    pub command: ExecutionNodeCommandProjection,
    pub replayed: bool,
}

/// Executions-owned acknowledgement projection for one Runtime Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionNodeCommandAcknowledgement {
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

/// Executions-owned Runtime observation projection for task convergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRuntimeObservationProjection {
    pub node_id: NodeId,
    pub command_id: Option<NodeCommandId>,
    pub received_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

/// Executions-owned port for Node command enqueue/reload/ack and Runtime
/// observation reads required by Execution Flow. Fleet remains the sole
/// Node-command authority behind the ACA.
#[async_trait]
pub trait IExecutionNodeCommandPort: Send + Sync {
    async fn enqueue_command(
        &self,
        request: ExecutionNodeCommandEnqueueRequest,
    ) -> Result<ExecutionNodeCommandDispatch, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<ExecutionNodeCommandProjection>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<ExecutionNodeCommandAcknowledgement>, RepositoryError>;

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<ExecutionRuntimeObservationProjection>, RepositoryError>;
}
