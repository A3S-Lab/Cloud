use crate::modules::executions::application::{
    ExecutionNodeCommandAcknowledgement, ExecutionNodeCommandDispatch,
    ExecutionNodeCommandEnqueueRequest, ExecutionNodeCommandProjection,
    ExecutionRuntimeObservationProjection, IExecutionNodeCommandPort,
};
use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use async_trait::async_trait;
use std::sync::Arc;

/// Anti-corruption adapter: Executions Execution Flow → Fleet Node-command store.
///
/// Translates Executions-owned enqueue/reload/ack/observation intents into Fleet
/// `INodeControlRepository` calls. No concrete persistence or lifecycle here.
#[derive(Clone)]
pub struct FleetExecutionNodeCommandAccessAdapter {
    node_control: Arc<dyn INodeControlRepository>,
}

impl FleetExecutionNodeCommandAccessAdapter {
    pub fn new(node_control: Arc<dyn INodeControlRepository>) -> Self {
        Self { node_control }
    }
}

#[async_trait]
impl IExecutionNodeCommandPort for FleetExecutionNodeCommandAccessAdapter {
    async fn enqueue_command(
        &self,
        request: ExecutionNodeCommandEnqueueRequest,
    ) -> Result<ExecutionNodeCommandDispatch, RepositoryError> {
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
        Ok(ExecutionNodeCommandDispatch {
            command: ExecutionNodeCommandProjection {
                id: write.value.id,
                node_id: write.value.node_id,
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
    ) -> Result<Option<ExecutionNodeCommandProjection>, RepositoryError> {
        Ok(self
            .node_control
            .find_command(node_id, command_id)
            .await?
            .map(|command| ExecutionNodeCommandProjection {
                id: command.id,
                node_id: command.node_id,
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
    ) -> Result<Option<ExecutionNodeCommandAcknowledgement>, RepositoryError> {
        Ok(self
            .node_control
            .command_acknowledgement(node_id, command_id)
            .await?
            .map(|acknowledgement| ExecutionNodeCommandAcknowledgement {
                completed_at: acknowledgement.completed_at,
                outcome: acknowledgement.outcome,
            }))
    }

    async fn latest_runtime_observation(
        &self,
        node_id: NodeId,
        unit_id: &str,
        generation: u64,
    ) -> Result<Option<ExecutionRuntimeObservationProjection>, RepositoryError> {
        Ok(self
            .node_control
            .latest_runtime_observation(node_id, unit_id, generation)
            .await?
            .map(|record| ExecutionRuntimeObservationProjection {
                node_id: record.node_id,
                command_id: record.command_id,
                received_at: record.received_at,
                observation: record.observation,
            }))
    }
}
