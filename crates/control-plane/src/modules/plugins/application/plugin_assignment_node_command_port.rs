use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Plugins-owned projection of one durable Node command used by Plugin Assignment Flow.
///
/// Fleet remains the command-store authority. Plugins only sees the consumer-facing
/// fields required to enqueue, reload, and validate Plugin Host commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAssignmentNodeCommandProjection {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

impl PluginAssignmentNodeCommandProjection {
    pub fn generation(&self) -> u64 {
        self.payload.generation()
    }
}

/// Plugins-owned enqueue request for one Plugin Host Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAssignmentNodeCommandEnqueueRequest {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Plugins-owned enqueue dispatch fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAssignmentNodeCommandDispatch {
    pub command: PluginAssignmentNodeCommandProjection,
    pub replayed: bool,
}

/// Plugins-owned acknowledgement projection for one Plugin Host Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAssignmentNodeCommandAcknowledgement {
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

/// Plugins-owned port for Node command enqueue/reload/ack required by Plugin
/// Assignment Flow. Fleet remains the sole Node-command authority behind the ACA.
#[async_trait]
pub trait IPluginAssignmentNodeCommandPort: Send + Sync {
    async fn enqueue_command(
        &self,
        request: PluginAssignmentNodeCommandEnqueueRequest,
    ) -> Result<PluginAssignmentNodeCommandDispatch, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<PluginAssignmentNodeCommandProjection>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<PluginAssignmentNodeCommandAcknowledgement>, RepositoryError>;
}
