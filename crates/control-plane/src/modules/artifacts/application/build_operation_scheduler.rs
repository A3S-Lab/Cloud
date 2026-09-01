use crate::modules::shared_kernel::domain::{
    BuildRunId, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Minimal Artifacts intent required to schedule one BuildRun operation.
///
/// Operations owns workflow execution and persistence. Artifacts owns the
/// meaning of a BuildRun, so its Application layer emits this consumer-shaped
/// intent instead of constructing Operations aggregates or repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildOperationRequest {
    operation_id: OperationId,
    organization_id: OrganizationId,
    build_run_id: BuildRunId,
    requested_at: DateTime<Utc>,
}

impl BuildOperationRequest {
    pub(crate) const fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        build_run_id: BuildRunId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            build_run_id,
            requested_at,
        }
    }

    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn build_run_id(self) -> BuildRunId {
        self.build_run_id
    }

    pub const fn requested_at(self) -> DateTime<Utc> {
        self.requested_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildOperationScheduleOutcome {
    replayed: bool,
}

impl BuildOperationScheduleOutcome {
    pub(crate) const fn new(replayed: bool) -> Self {
        Self { replayed }
    }

    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[async_trait]
pub trait IBuildOperationScheduler: Send + Sync {
    async fn schedule(
        &self,
        request: BuildOperationRequest,
    ) -> Result<BuildOperationScheduleOutcome, RepositoryError>;
}
