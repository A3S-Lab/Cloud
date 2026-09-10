use crate::modules::shared_kernel::domain::{
    ExecutionId, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Minimal Executions intent required to schedule one Execution operation.
///
/// Operations owns workflow execution and persistence. Executions owns the
/// meaning of an Execution, so its Application layer emits this consumer-shaped
/// intent instead of constructing Operations aggregates or repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionOperationRequest {
    operation_id: OperationId,
    organization_id: OrganizationId,
    execution_id: ExecutionId,
    requested_at: DateTime<Utc>,
}

impl ExecutionOperationRequest {
    pub(crate) const fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        execution_id: ExecutionId,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            execution_id,
            requested_at,
        }
    }

    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn execution_id(self) -> ExecutionId {
        self.execution_id
    }

    pub const fn requested_at(self) -> DateTime<Utc> {
        self.requested_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionOperationScheduleOutcome {
    replayed: bool,
}

impl ExecutionOperationScheduleOutcome {
    pub(crate) const fn new(replayed: bool) -> Self {
        Self { replayed }
    }

    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[async_trait]
pub trait IExecutionOperationScheduler: Send + Sync {
    async fn schedule(
        &self,
        request: ExecutionOperationRequest,
    ) -> Result<ExecutionOperationScheduleOutcome, RepositoryError>;
}
