use crate::modules::agents::domain::{
    AgentExecutionCheckpoint, AgentExecutionEvent, AgentExecutionEventDraft,
    AgentExecutionEventKind, AgentExecutionWrite,
};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, IdempotencyRequest, OrganizationId,
    RepositoryError,
};
use a3s_cloud_contracts::DomainEventEnvelope;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
pub const MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_ORPHAN_GRACE_MS: u64 = 366 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone)]
pub struct CommitAgentExecutionCheckpointWrite {
    pub checkpoint: AgentExecutionCheckpoint,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
    pub object_lease_id: Option<Uuid>,
    pub committed_at: DateTime<Utc>,
}

impl CommitAgentExecutionCheckpointWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.checkpoint.validate()?;
        let checkpoint = &self.checkpoint;
        if self.event.event_key != "agent.execution-checkpoint.committed"
            || self.event.schema_version != 1
            || self.event.organization_id() != Some(checkpoint.organization_id.as_uuid())
            || self.event.aggregate_id != checkpoint.id.as_uuid()
            || self.event.aggregate_version != checkpoint.aggregate_version
            || self.event.occurred_at != checkpoint.captured_at
            || self.event.event_id.is_nil()
            || self.event.correlation_id.is_nil()
            || self
                .object_lease_id
                .is_some_and(|lease_id| lease_id.is_nil())
            || self.committed_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.committed_at)
            || self.committed_at < checkpoint.captured_at
        {
            return Err("Agent checkpoint event does not match its projection".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentExecutionCheckpointObjectLeasePurpose {
    Capture,
    Inventory,
    Cleanup,
}

impl AgentExecutionCheckpointObjectLeasePurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Inventory => "inventory",
            Self::Cleanup => "cleanup",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "capture" => Ok(Self::Capture),
            "inventory" => Ok(Self::Inventory),
            "cleanup" => Ok(Self::Cleanup),
            _ => Err(format!(
                "unknown Agent checkpoint object lease purpose {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointObjectLease {
    pub reference: crate::modules::agents::domain::AgentExecutionCheckpointObjectReference,
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentExecutionCheckpointId,
    pub purpose: AgentExecutionCheckpointObjectLeasePurpose,
    pub lease_id: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

impl AgentExecutionCheckpointObjectLease {
    pub fn validate(&self) -> Result<(), String> {
        let identity = self.reference.identity()?;
        if identity.organization_id != self.organization_id
            || identity.execution_id != self.execution_id
            || identity.checkpoint_id != self.checkpoint_id
            || identity.digest != self.reference.digest
            || self.lease_id.is_nil()
            || self.reserved_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.reserved_at)
            || self.lease_expires_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.lease_expires_at)
            || self.lease_expires_at <= self.reserved_at
            || self.lease_expires_at - self.reserved_at
                > Duration::milliseconds(
                    MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_ORPHAN_GRACE_MS as i64,
                )
        {
            return Err("Agent checkpoint object lease is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ReserveAgentExecutionCheckpointObjectWrite {
    pub checkpoint: AgentExecutionCheckpoint,
    pub reserved_at: DateTime<Utc>,
    pub lease_duration: Duration,
}

impl ReserveAgentExecutionCheckpointObjectWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.checkpoint.validate()?;
        validate_lease_window(
            self.reserved_at,
            self.lease_duration,
            Duration::milliseconds(MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS as i64),
        )
    }

    pub fn lease_expires_at(&self) -> Result<DateTime<Utc>, String> {
        lease_expires_at(self.reserved_at, self.lease_duration)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentExecutionCheckpointObjectCaptureReservation {
    Committed(Box<AgentExecutionCheckpoint>),
    Reserved(Box<AgentExecutionCheckpointObjectLease>),
}

#[derive(Debug, Clone)]
pub struct ReconcileAgentExecutionCheckpointObjectWrite {
    pub reference: crate::modules::agents::domain::AgentExecutionCheckpointObjectReference,
    pub observed_at: DateTime<Utc>,
    pub orphan_grace: Duration,
    pub cleanup_lease_duration: Duration,
}

impl ReconcileAgentExecutionCheckpointObjectWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.reference.validate()?;
        validate_lease_window(
            self.observed_at,
            self.orphan_grace,
            Duration::milliseconds(MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_ORPHAN_GRACE_MS as i64),
        )?;
        validate_lease_window(
            self.observed_at,
            self.cleanup_lease_duration,
            Duration::milliseconds(MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS as i64),
        )
    }

    pub fn observation_expires_at(&self) -> Result<DateTime<Utc>, String> {
        lease_expires_at(self.observed_at, self.orphan_grace)
    }

    pub fn cleanup_expires_at(&self) -> Result<DateTime<Utc>, String> {
        lease_expires_at(self.observed_at, self.cleanup_lease_duration)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentExecutionCheckpointObjectReconcileDisposition {
    Referenced,
    Deferred { retry_not_before: DateTime<Utc> },
    CleanupClaimed(Box<AgentExecutionCheckpointObjectLease>),
}

#[derive(Debug, Clone)]
pub struct ClaimExpiredAgentExecutionCheckpointObjectsWrite {
    pub claimed_at: DateTime<Utc>,
    pub cleanup_lease_duration: Duration,
    pub limit: usize,
}

impl ClaimExpiredAgentExecutionCheckpointObjectsWrite {
    pub fn validate(&self) -> Result<(), String> {
        validate_lease_window(
            self.claimed_at,
            self.cleanup_lease_duration,
            Duration::milliseconds(MAX_AGENT_EXECUTION_CHECKPOINT_OBJECT_LEASE_MS as i64),
        )?;
        if self.limit == 0 || self.limit > 1_000 {
            return Err("Agent checkpoint cleanup claim limit is invalid".into());
        }
        Ok(())
    }

    pub fn cleanup_expires_at(&self) -> Result<DateTime<Utc>, String> {
        lease_expires_at(self.claimed_at, self.cleanup_lease_duration)
    }
}

#[derive(Debug, Clone)]
pub struct CompleteAgentExecutionCheckpointObjectCleanupWrite {
    pub lease: AgentExecutionCheckpointObjectLease,
    pub completed_at: DateTime<Utc>,
}

impl CompleteAgentExecutionCheckpointObjectCleanupWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.lease.validate()?;
        if self.lease.purpose != AgentExecutionCheckpointObjectLeasePurpose::Cleanup
            || self.completed_at
                != crate::modules::shared_kernel::domain::canonical_timestamp(self.completed_at)
            || self.completed_at < self.lease.reserved_at
        {
            return Err("Agent checkpoint object cleanup completion is invalid".into());
        }
        Ok(())
    }
}

fn validate_lease_window(
    started_at: DateTime<Utc>,
    duration: Duration,
    maximum: Duration,
) -> Result<(), String> {
    if started_at != crate::modules::shared_kernel::domain::canonical_timestamp(started_at)
        || duration <= Duration::zero()
        || duration > maximum
    {
        return Err("Agent checkpoint object lease window is invalid".into());
    }
    lease_expires_at(started_at, duration).map(|_| ())
}

fn lease_expires_at(
    started_at: DateTime<Utc>,
    duration: Duration,
) -> Result<DateTime<Utc>, String> {
    started_at
        .checked_add_signed(duration)
        .map(crate::modules::shared_kernel::domain::canonical_timestamp)
        .ok_or_else(|| "Agent checkpoint object lease expiration overflowed".to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionCheckpointWrite {
    pub checkpoint: AgentExecutionCheckpoint,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct ForkAgentExecutionWrite {
    pub execution: crate::modules::agents::domain::AgentExecution,
    pub initial_event: AgentExecutionEventDraft,
    pub event: DomainEventEnvelope,
    pub idempotency: IdempotencyRequest,
}

impl ForkAgentExecutionWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.execution.validate()?;
        self.initial_event.content.validate()?;
        let lineage = self
            .execution
            .lineage
            .as_ref()
            .ok_or_else(|| "forked Agent execution has no lineage".to_owned())?;
        if self.initial_event.kind != AgentExecutionEventKind::ExecutionRequested
            || self.initial_event.occurred_at != self.execution.requested_at
            || self.execution.aggregate_version != 1
            || lineage.parent_execution_id == self.execution.id
            || self.event.event_key != "agent.execution.forked"
            || self.event.schema_version != 1
            || self.event.organization_id() != Some(self.execution.organization_id.as_uuid())
            || self.event.aggregate_id != self.execution.id.as_uuid()
            || self.event.aggregate_version != self.execution.aggregate_version
            || self.event.occurred_at != self.execution.requested_at
            || self.event.event_id.is_nil()
            || self.event.correlation_id.is_nil()
        {
            return Err("Agent execution fork event does not match its aggregate".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentExecutionCheckpointWriteReference {
    pub organization_id: OrganizationId,
    pub checkpoint_id: AgentExecutionCheckpointId,
}

#[async_trait]
pub trait IAgentExecutionCheckpointRepository: Send + Sync {
    async fn reserve_execution_checkpoint_object(
        &self,
        write: ReserveAgentExecutionCheckpointObjectWrite,
    ) -> Result<AgentExecutionCheckpointObjectCaptureReservation, RepositoryError>;

    async fn commit_execution_checkpoint(
        &self,
        write: CommitAgentExecutionCheckpointWrite,
    ) -> Result<AgentExecutionCheckpointWrite, RepositoryError>;

    async fn fork_execution(
        &self,
        write: ForkAgentExecutionWrite,
    ) -> Result<AgentExecutionWrite, RepositoryError>;

    async fn replay_execution_checkpoint(
        &self,
        idempotency: &IdempotencyRequest,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError>;

    async fn find_execution_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AgentExecutionCheckpointId,
    ) -> Result<Option<AgentExecutionCheckpoint>, RepositoryError>;

    async fn list_execution_checkpoints(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        limit: usize,
    ) -> Result<Vec<AgentExecutionCheckpoint>, RepositoryError>;

    async fn list_execution_trajectory_events(
        &self,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
        after_sequence: Option<u64>,
        through_sequence: Option<u64>,
        limit: usize,
    ) -> Result<Vec<AgentExecutionEvent>, RepositoryError>;

    async fn reconcile_execution_checkpoint_object(
        &self,
        write: ReconcileAgentExecutionCheckpointObjectWrite,
    ) -> Result<AgentExecutionCheckpointObjectReconcileDisposition, RepositoryError>;

    async fn claim_expired_execution_checkpoint_objects(
        &self,
        write: ClaimExpiredAgentExecutionCheckpointObjectsWrite,
    ) -> Result<Vec<AgentExecutionCheckpointObjectLease>, RepositoryError>;

    async fn complete_execution_checkpoint_object_cleanup(
        &self,
        write: CompleteAgentExecutionCheckpointObjectCleanupWrite,
    ) -> Result<(), RepositoryError>;
}
