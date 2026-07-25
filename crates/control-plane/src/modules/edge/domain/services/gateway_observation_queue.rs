use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayRolloutId, NodeCommandId, NodeId, RepositoryError,
};
use a3s_cloud_contracts::{GatewaySnapshotObservationRequest, NodeGatewaySnapshotObservation};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayObservationCommand {
    pub rollout_id: GatewayRolloutId,
    pub correlation_id: Uuid,
    pub node_id: NodeId,
    pub candidate_revision: u64,
    pub candidate_snapshot_digest: String,
    pub command_id: NodeCommandId,
    pub attempt: u32,
    pub issued_at: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
}

impl GatewayObservationCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rollout_id: GatewayRolloutId,
        correlation_id: Uuid,
        node_id: NodeId,
        candidate_revision: u64,
        candidate_snapshot_digest: impl Into<String>,
        command_id: NodeCommandId,
        attempt: u32,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<Self, String> {
        let command = Self {
            rollout_id,
            correlation_id,
            node_id,
            candidate_revision,
            candidate_snapshot_digest: candidate_snapshot_digest.into(),
            command_id,
            attempt,
            issued_at: canonical_timestamp(issued_at),
            not_after: canonical_timestamp(not_after),
        };
        command.validate()?;
        Ok(command)
    }

    pub fn request(&self) -> Result<GatewaySnapshotObservationRequest, String> {
        self.validate()?;
        GatewaySnapshotObservationRequest::new(
            self.node_id.as_uuid(),
            self.candidate_revision,
            self.candidate_snapshot_digest.clone(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rollout_id.as_uuid().is_nil()
            || self.correlation_id.is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.attempt == 0
            || self.not_after <= self.issued_at
        {
            return Err("Gateway observation command identity and validity are invalid".into());
        }
        GatewaySnapshotObservationRequest::new(
            self.node_id.as_uuid(),
            self.candidate_revision,
            self.candidate_snapshot_digest.clone(),
        )
        .map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GatewayObservationDispatch {
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayObservationCommandOutcome {
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

#[async_trait]
pub trait IGatewayObservationQueue: Send + Sync {
    async fn enqueue(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<GatewayObservationDispatch, RepositoryError>;

    async fn outcome(
        &self,
        command: &GatewayObservationCommand,
    ) -> Result<Option<GatewayObservationCommandOutcome>, RepositoryError>;
}
