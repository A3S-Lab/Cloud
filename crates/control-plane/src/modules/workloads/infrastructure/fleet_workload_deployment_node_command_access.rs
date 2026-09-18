use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use crate::modules::workloads::application::{
    IWorkloadDeploymentNodeCommandPort, WorkloadDeploymentNodeCommandAcknowledgement,
    WorkloadDeploymentNodeCommandDispatch, WorkloadDeploymentNodeCommandEnqueueRequest,
    WorkloadDeploymentNodeCommandProjection, WorkloadDeploymentResourceInventoryProjection,
    WorkloadDeploymentRuntimeObservationProjection,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter: Workloads Deployment Flow → Fleet Node-command store.
///
/// Translates Workloads-owned enqueue/reload/ack/observation/inventory intents into
/// Fleet `INodeControlRepository` calls. No concrete persistence or lifecycle here.
#[derive(Clone)]
pub struct FleetWorkloadDeploymentNodeCommandAccessAdapter {
    node_control: Arc<dyn INodeControlRepository>,
}

impl FleetWorkloadDeploymentNodeCommandAccessAdapter {
    pub fn new(node_control: Arc<dyn INodeControlRepository>) -> Self {
        Self { node_control }
    }
}

#[async_trait]
impl IWorkloadDeploymentNodeCommandPort for FleetWorkloadDeploymentNodeCommandAccessAdapter {
    async fn enqueue_command(
        &self,
        request: WorkloadDeploymentNodeCommandEnqueueRequest,
    ) -> Result<WorkloadDeploymentNodeCommandDispatch, RepositoryError> {
        let write = self
            .node_control
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: request.proposed_command_id,
                node_id: request.node_id,
                aggregate_id: request.aggregate_id,
                payload: request.payload,
                issued_at: request.issued_at,
                not_after: request.not_after,
                correlation_id: request.correlation_id,
            })
            .await?;
        Ok(WorkloadDeploymentNodeCommandDispatch {
            command: WorkloadDeploymentNodeCommandProjection {
                id: write.value.id,
                node_id: write.value.node_id,
                sequence: write.value.sequence,
                aggregate_id: write.value.aggregate_id,
                payload: write.value.payload,
                issued_at: write.value.issued_at,
                not_after: write.value.not_after,
                correlation_id: write.value.correlation_id,
            },
            replayed: write.replayed,
        })
    }

    async fn find_command(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<WorkloadDeploymentNodeCommandProjection>, RepositoryError> {
        Ok(self
            .node_control
            .find_command(node_id, command_id)
            .await?
            .map(|command| WorkloadDeploymentNodeCommandProjection {
                id: command.id,
                node_id: command.node_id,
                sequence: command.sequence,
                aggregate_id: command.aggregate_id,
                payload: command.payload,
                issued_at: command.issued_at,
                not_after: command.not_after,
                correlation_id: command.correlation_id,
            }))
    }

    async fn command_acknowledgement(
        &self,
        node_id: NodeId,
        command_id: NodeCommandId,
    ) -> Result<Option<WorkloadDeploymentNodeCommandAcknowledgement>, RepositoryError> {
        Ok(self
            .node_control
            .command_acknowledgement(node_id, command_id)
            .await?
            .map(|acknowledgement| WorkloadDeploymentNodeCommandAcknowledgement {
                lease_id: acknowledgement.lease_id,
                completed_at: acknowledgement.completed_at,
                outcome: acknowledgement.outcome,
            }))
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<WorkloadDeploymentRuntimeObservationProjection>, RepositoryError> {
        Ok(self
            .node_control
            .latest_runtime_observation(node_id, unit_id, generation)
            .await?
            .map(|record| WorkloadDeploymentRuntimeObservationProjection {
                report_id: record.report_id,
                node_id: record.node_id,
                command_id: record.command_id,
                observed_at: record.observed_at,
                received_at: record.received_at,
                observation: record.observation,
            }))
    }

    async fn current_resource_inventory(
        &self,
        node_id: NodeId,
    ) -> Result<Option<WorkloadDeploymentResourceInventoryProjection>, RepositoryError> {
        Ok(self
            .node_control
            .current_resource_inventory(node_id)
            .await?
            .map(|record| WorkloadDeploymentResourceInventoryProjection {
                inventory: record.inventory,
                received_at: record.received_at,
            }))
    }
}
