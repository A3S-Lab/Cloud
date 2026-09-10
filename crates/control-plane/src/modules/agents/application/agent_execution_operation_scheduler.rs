use crate::modules::shared_kernel::domain::{
    AgentExecutionId, OperationId, OrganizationId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Minimal Agents intent required to schedule one AgentExecution operation.
///
/// Operations owns workflow execution and persistence. Agents owns the meaning
/// of an AgentExecution, so its Application layer emits this consumer-shaped
/// intent instead of constructing Operations aggregates or repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentExecutionOperationRequest {
    operation_id: OperationId,
    organization_id: OrganizationId,
    execution_id: AgentExecutionId,
    requested_at: DateTime<Utc>,
}

impl AgentExecutionOperationRequest {
    pub(crate) const fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        execution_id: AgentExecutionId,
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

    pub const fn execution_id(self) -> AgentExecutionId {
        self.execution_id
    }

    pub const fn requested_at(self) -> DateTime<Utc> {
        self.requested_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentExecutionOperationScheduleOutcome {
    replayed: bool,
}

impl AgentExecutionOperationScheduleOutcome {
    pub(crate) const fn new(replayed: bool) -> Self {
        Self { replayed }
    }

    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[async_trait]
pub trait IAgentExecutionOperationScheduler: Send + Sync {
    async fn schedule(
        &self,
        request: AgentExecutionOperationRequest,
    ) -> Result<AgentExecutionOperationScheduleOutcome, RepositoryError>;
}
