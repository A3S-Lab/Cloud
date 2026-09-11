use crate::modules::fleet::domain::entities::NodeCommandDraft;
use crate::modules::fleet::domain::repositories::INodeControlRepository;
use crate::modules::shared_kernel::domain::{NodeCommandId, NodeId, RepositoryError};
use a3s_cloud_contracts::{
    GatewaySnapshot, GatewaySnapshotObservationRequest, NodeCommandOutcome, NodeCommandPayload,
    NodeCommandResult, NodeGatewaySnapshotObservation,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// Fleet-owned install request for one Gateway snapshot command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetGatewaySnapshotInstallRequest {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub correlation_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub snapshot: GatewaySnapshot,
}

impl FleetGatewaySnapshotInstallRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.correlation_id.is_nil()
            || self.not_after <= self.issued_at
        {
            return Err("Fleet Gateway snapshot install request is invalid".into());
        }
        Ok(())
    }
}

/// Fleet-owned observe request for one Gateway snapshot observation command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetGatewaySnapshotObserveRequest {
    pub node_id: NodeId,
    pub command_id: NodeCommandId,
    pub correlation_id: Uuid,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub aggregate_id: Uuid,
    pub request: GatewaySnapshotObservationRequest,
}

impl FleetGatewaySnapshotObserveRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.node_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.correlation_id.is_nil()
            || self.aggregate_id.is_nil()
            || self.not_after <= self.issued_at
        {
            return Err("Fleet Gateway snapshot observe request is invalid".into());
        }
        self.request.validate()?;
        Ok(())
    }
}

/// Fleet-owned enqueue dispatch fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FleetGatewayCommandDispatch {
    pub replayed: bool,
}

/// Fleet-owned observation command outcome for Gateway snapshot observe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetGatewayObservationOutcome {
    Observed {
        observation: Box<NodeGatewaySnapshotObservation>,
        completed_at: DateTime<Utc>,
    },
    Failed {
        failure: String,
        retryable: bool,
        completed_at: DateTime<Utc>,
    },
}

/// Fleet owner port for Gateway snapshot install/observe Node commands.
#[async_trait]
pub trait IFleetGatewaySnapshotCommandPort: Send + Sync {
    async fn enqueue_install(
        &self,
        request: FleetGatewaySnapshotInstallRequest,
    ) -> Result<FleetGatewayCommandDispatch, RepositoryError>;

    async fn enqueue_observe(
        &self,
        request: FleetGatewaySnapshotObserveRequest,
    ) -> Result<FleetGatewayCommandDispatch, RepositoryError>;

    async fn observation_outcome(
        &self,
        request: &FleetGatewaySnapshotObserveRequest,
    ) -> Result<Option<FleetGatewayObservationOutcome>, RepositoryError>;
}

/// Fleet owner-side service. It is the sole component that interprets Gateway
/// snapshot install/observe intents into NodeCommandDraft authority.
pub struct FleetGatewaySnapshotCommandService {
    commands: Arc<dyn INodeControlRepository>,
}

impl FleetGatewaySnapshotCommandService {
    pub fn new(commands: Arc<dyn INodeControlRepository>) -> Self {
        Self { commands }
    }
}

#[async_trait]
impl IFleetGatewaySnapshotCommandPort for FleetGatewaySnapshotCommandService {
    async fn enqueue_install(
        &self,
        request: FleetGatewaySnapshotInstallRequest,
    ) -> Result<FleetGatewayCommandDispatch, RepositoryError> {
        request
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let result = self
            .commands
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: request.command_id,
                node_id: request.node_id,
                aggregate_id: request.node_id.as_uuid(),
                payload: NodeCommandPayload::GatewaySnapshotInstall {
                    snapshot: Box::new(request.snapshot),
                },
                issued_at: request.issued_at,
                not_after: request.not_after,
                correlation_id: request.correlation_id,
            })
            .await?;
        Ok(FleetGatewayCommandDispatch {
            replayed: result.replayed,
        })
    }

    async fn enqueue_observe(
        &self,
        request: FleetGatewaySnapshotObserveRequest,
    ) -> Result<FleetGatewayCommandDispatch, RepositoryError> {
        request
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let result = self
            .commands
            .enqueue_command(NodeCommandDraft {
                proposed_command_id: request.command_id,
                node_id: request.node_id,
                aggregate_id: request.aggregate_id,
                payload: NodeCommandPayload::GatewaySnapshotObserve {
                    request: request.request,
                },
                issued_at: request.issued_at,
                not_after: request.not_after,
                correlation_id: request.correlation_id,
            })
            .await?;
        Ok(FleetGatewayCommandDispatch {
            replayed: result.replayed,
        })
    }

    async fn observation_outcome(
        &self,
        request: &FleetGatewaySnapshotObserveRequest,
    ) -> Result<Option<FleetGatewayObservationOutcome>, RepositoryError> {
        request
            .validate()
            .map_err(|error| RepositoryError::Conflict(error))?;
        let Some(acknowledgement) = self
            .commands
            .command_acknowledgement(request.node_id, request.command_id)
            .await?
        else {
            return Ok(None);
        };
        if acknowledgement.command_id != request.command_id.as_uuid()
            || acknowledgement.node_id != request.node_id.as_uuid()
        {
            return Err(RepositoryError::Storage(
                "Gateway observation acknowledgement identity is inconsistent".into(),
            ));
        }
        match acknowledgement.outcome {
            NodeCommandOutcome::Succeeded { result } => match *result {
                NodeCommandResult::GatewaySnapshotObserved { observation } => {
                    observation
                        .validate_for(
                            request.command_id.as_uuid(),
                            request.node_id.as_uuid(),
                            &request.request,
                        )
                        .map_err(RepositoryError::Storage)?;
                    Ok(Some(FleetGatewayObservationOutcome::Observed {
                        observation: Box::new(observation),
                        completed_at: acknowledgement.completed_at,
                    }))
                }
                _ => Err(RepositoryError::Storage(
                    "Gateway observation command stored an incompatible successful result".into(),
                )),
            },
            NodeCommandOutcome::Rejected { failure } => {
                Ok(Some(FleetGatewayObservationOutcome::Failed {
                    failure: bounded_failure("rejected", &failure.code),
                    retryable: failure.retryable,
                    completed_at: acknowledgement.completed_at,
                }))
            }
            NodeCommandOutcome::Failed { failure } => {
                Ok(Some(FleetGatewayObservationOutcome::Failed {
                    failure: bounded_failure("failed", &failure.code),
                    retryable: failure.retryable,
                    completed_at: acknowledgement.completed_at,
                }))
            }
        }
    }
}

fn bounded_failure(outcome: &str, code: &str) -> String {
    format!("Gateway observation command {outcome} with code {code}")
}
