use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Artifacts-owned projection of one durable Node command used by Build Flow.
///
/// Fleet remains the command-store authority. Artifacts only sees the consumer-facing
/// fields required to enqueue, reload, and validate Box build commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBuildNodeCommandProjection {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Artifacts-owned enqueue request for one Box build Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBuildNodeCommandEnqueueRequest {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Artifacts-owned enqueue dispatch fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBuildNodeCommandDispatch {
    pub command: ArtifactBuildNodeCommandProjection,
    pub replayed: bool,
}

/// Artifacts-owned acknowledgement projection for one Box build Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBuildNodeCommandAcknowledgement {
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

/// Artifacts-owned port for Node command enqueue/reload/ack required by Build Flow.
/// Fleet remains the sole Node-command authority behind the ACA.
#[async_trait]
pub trait IArtifactBuildNodeCommandPort: Send + Sync {
    async fn enqueue_command(
        &self,
        request: ArtifactBuildNodeCommandEnqueueRequest,
    ) -> Result<ArtifactBuildNodeCommandDispatch, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<ArtifactBuildNodeCommandProjection>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<ArtifactBuildNodeCommandAcknowledgement>, RepositoryError>;
}
