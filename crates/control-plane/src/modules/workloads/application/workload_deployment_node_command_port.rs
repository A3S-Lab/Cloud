use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{NodeCommandOutcome, NodeCommandPayload, NodeResourceInventory};
use a3s_runtime::contract::RuntimeObservation;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Workloads-owned projection of one durable Node command used by Deployment Flow.
///
/// Fleet remains the command-store authority. Workloads only sees the consumer-facing
/// fields required to enqueue, reload, and validate Runtime / resource-claim commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentNodeCommandProjection {
    pub id: NodeCommandId,
    pub node_id: NodeId,
    pub sequence: u64,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Workloads-owned enqueue request for one Deployment Flow Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentNodeCommandEnqueueRequest {
    pub proposed_command_id: NodeCommandId,
    pub node_id: NodeId,
    pub aggregate_id: Uuid,
    pub payload: NodeCommandPayload,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub correlation_id: Uuid,
}

/// Workloads-owned enqueue dispatch fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentNodeCommandDispatch {
    pub command: WorkloadDeploymentNodeCommandProjection,
    pub replayed: bool,
}

/// Workloads-owned acknowledgement projection for one Deployment Flow Node command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentNodeCommandAcknowledgement {
    pub lease_id: Uuid,
    pub completed_at: DateTime<Utc>,
    pub outcome: NodeCommandOutcome,
}

/// Workloads-owned Runtime observation projection for Deployment Flow convergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentRuntimeObservationProjection {
    pub report_id: Uuid,
    pub node_id: NodeId,
    pub command_id: Option<NodeCommandId>,
    pub observed_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    pub observation: RuntimeObservation,
}

/// Workloads-owned resource inventory projection for Deployment Flow scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadDeploymentResourceInventoryProjection {
    pub inventory: NodeResourceInventory,
    pub received_at: DateTime<Utc>,
}

impl WorkloadDeploymentResourceInventoryProjection {
    pub fn validate(&self) -> Result<(), String> {
        self.inventory.validate()
    }
}

impl WorkloadDeploymentNodeCommandProjection {
    pub fn payload_digest(&self) -> Result<String, String> {
        self.payload.digest()
    }

    pub fn envelope(
        &self,
        lease_id: Uuid,
    ) -> Result<a3s_cloud_contracts::NodeCommandEnvelope, String> {
        a3s_cloud_contracts::NodeCommandEnvelope::new(
            a3s_cloud_contracts::NodeCommandMetadata {
                command_id: self.id.as_uuid(),
                lease_id,
                node_id: self.node_id.as_uuid(),
                sequence: self.sequence,
                aggregate_id: self.aggregate_id,
                issued_at: self.issued_at,
                not_after: self.not_after,
                correlation_id: self.correlation_id,
            },
            self.payload.clone(),
        )
    }
}

/// Workloads-owned port for Node command enqueue/reload/ack plus the inventory and
/// Runtime observation reads required by Deployment Flow. Fleet remains the sole
/// Node-command authority behind the ACA.
#[async_trait]
pub trait IWorkloadDeploymentNodeCommandPort: Send + Sync {
    async fn enqueue_command(
        &self,
        request: WorkloadDeploymentNodeCommandEnqueueRequest,
    ) -> Result<WorkloadDeploymentNodeCommandDispatch, RepositoryError>;

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<WorkloadDeploymentNodeCommandProjection>, RepositoryError>;

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<WorkloadDeploymentNodeCommandAcknowledgement>, RepositoryError>;

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<WorkloadDeploymentRuntimeObservationProjection>, RepositoryError>;

    async fn current_resource_inventory(
        &self,
        node_id: NodeId,
    ) -> Result<Option<WorkloadDeploymentResourceInventoryProjection>, RepositoryError>;
}
