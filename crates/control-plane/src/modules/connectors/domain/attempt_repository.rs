use super::{
    ConnectorExecutionAttempt, ConnectorExecutionAttemptBinding, ConnectorExecutionAttemptCursor,
    ConnectorExecutionAttemptRecord, ConnectorExecutionEvidence, ConnectorExecutionFence,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, IdempotentWrite,
    OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use uuid::Uuid;

use super::{
    MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS, MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS,
};

#[derive(Clone, PartialEq, Eq)]
pub struct ReserveConnectorExecutionAttempt {
    pub binding: ConnectorExecutionAttemptBinding,
    pub fence_token: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

impl ReserveConnectorExecutionAttempt {
    pub fn new(
        binding: ConnectorExecutionAttemptBinding,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let request = Self {
            binding,
            fence_token,
            reserved_at: canonical_timestamp(reserved_at),
            lease_expires_at: canonical_timestamp(lease_expires_at),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.fence_token.is_nil()
            || self.reserved_at != canonical_timestamp(self.reserved_at)
            || self.lease_expires_at != canonical_timestamp(self.lease_expires_at)
            || self.lease_expires_at <= self.reserved_at
            || self.lease_expires_at - self.reserved_at
                > Duration::seconds(MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS)
        {
            return Err("Connector execution reservation request is invalid".into());
        }
        Ok(())
    }
}

impl fmt::Debug for ReserveConnectorExecutionAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReserveConnectorExecutionAttempt")
            .field("binding", &self.binding)
            .field("fence_token", &"redacted")
            .field("reserved_at", &self.reserved_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectorExecutionReservation {
    Acquired {
        fence: ConnectorExecutionFence,
        replayed: bool,
    },
    Busy(ConnectorExecutionAttemptRecord),
    InFlight(ConnectorExecutionAttemptRecord),
    Indeterminate(ConnectorExecutionAttemptRecord),
    Completed(ConnectorExecutionAttemptRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginConnectorExecutionDispatch {
    pub fence: ConnectorExecutionFence,
    pub dispatch_started_at: DateTime<Utc>,
    pub outcome_deadline_at: DateTime<Utc>,
}

impl BeginConnectorExecutionDispatch {
    pub fn new(
        fence: ConnectorExecutionFence,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let request = Self {
            fence,
            dispatch_started_at: canonical_timestamp(dispatch_started_at),
            outcome_deadline_at: canonical_timestamp(outcome_deadline_at),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.fence.validate()?;
        if self.dispatch_started_at != canonical_timestamp(self.dispatch_started_at)
            || self.outcome_deadline_at != canonical_timestamp(self.outcome_deadline_at)
            || self.dispatch_started_at < self.fence.reserved_at()
            || self.dispatch_started_at >= self.fence.lease_expires_at()
            || self.outcome_deadline_at <= self.dispatch_started_at
            || self.outcome_deadline_at - self.dispatch_started_at
                > Duration::seconds(MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS)
        {
            return Err("Connector execution dispatch transition is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleConnectorExecutionAttempt {
    pub fence: ConnectorExecutionFence,
    pub evidence: ConnectorExecutionEvidence,
}

impl SettleConnectorExecutionAttempt {
    pub fn new(
        fence: ConnectorExecutionFence,
        evidence: ConnectorExecutionEvidence,
    ) -> Result<Self, String> {
        let settlement = Self { fence, evidence };
        settlement.validate()?;
        Ok(settlement)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.fence.validate()?;
        self.evidence.validate()?;
        if !self.fence.binding().matches_evidence(&self.evidence) {
            return Err("Connector execution settlement does not match its fence".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IConnectorExecutionAttemptRepository: Send + Sync {
    /// Atomically creates or acquires the pre-dispatch reservation.
    ///
    /// Only an expired `reserved` row may rotate the generation and token.
    /// `dispatching` rows are observation-only and never authorize another call.
    async fn reserve(
        &self,
        request: ReserveConnectorExecutionAttempt,
    ) -> Result<ConnectorExecutionReservation, RepositoryError>;

    /// Commits the one-way provider-call intent. This transition is deliberately
    /// non-replayable: an ambiguous commit must be recovered as dispatching, not
    /// interpreted as permission to issue another network request.
    async fn begin_dispatch(
        &self,
        request: BeginConnectorExecutionDispatch,
    ) -> Result<ConnectorExecutionAttemptRecord, RepositoryError>;

    /// Atomically records immutable terminal evidence and terminates the attempt.
    /// Replaying the same fence and evidence is idempotent; changed content conflicts.
    async fn settle(
        &self,
        request: SettleConnectorExecutionAttempt,
    ) -> Result<IdempotentWrite<ConnectorExecutionAttemptRecord>, RepositoryError>;

    #[allow(clippy::too_many_arguments)]
    async fn find(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
    ) -> Result<Option<ConnectorExecutionAttemptRecord>, RepositoryError>;

    /// Returns only non-terminal rows, ordered by creation time and attempt ID descending.
    #[allow(clippy::too_many_arguments)]
    async fn list_unresolved_page(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        after: Option<ConnectorExecutionAttemptCursor>,
        limit: usize,
    ) -> Result<Vec<ConnectorExecutionAttemptRecord>, RepositoryError>;
}

pub(crate) fn reservation_record(
    request: &ReserveConnectorExecutionAttempt,
    generation: u64,
    created_at: DateTime<Utc>,
) -> Result<ConnectorExecutionAttemptRecord, String> {
    ConnectorExecutionAttemptRecord::new(
        ConnectorExecutionAttempt::restore(
            request.binding.clone(),
            super::ConnectorExecutionAttemptState::Reserved,
            generation,
            request.fence_token,
            request.reserved_at,
            request.lease_expires_at,
            None,
            None,
            None,
            created_at,
        )?,
        None,
    )
}
