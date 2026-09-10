use crate::modules::shared_kernel::domain::{
    OperationId, OrganizationId, PluginAssignmentId, RepositoryError,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Minimal Plugins intent required to schedule one plugin-assignment operation.
///
/// Operations owns workflow execution and persistence. Plugins owns the meaning
/// of a PluginAssignment, so its Application layer emits this consumer-shaped
/// intent instead of constructing Operations aggregates or repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginAssignmentOperationRequest {
    operation_id: OperationId,
    organization_id: OrganizationId,
    assignment_id: PluginAssignmentId,
    assignment_generation: u64,
    requested_at: DateTime<Utc>,
}

impl PluginAssignmentOperationRequest {
    pub(crate) const fn new(
        operation_id: OperationId,
        organization_id: OrganizationId,
        assignment_id: PluginAssignmentId,
        assignment_generation: u64,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            operation_id,
            organization_id,
            assignment_id,
            assignment_generation,
            requested_at,
        }
    }

    pub const fn operation_id(self) -> OperationId {
        self.operation_id
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn assignment_id(self) -> PluginAssignmentId {
        self.assignment_id
    }

    pub const fn assignment_generation(self) -> u64 {
        self.assignment_generation
    }

    pub const fn requested_at(self) -> DateTime<Utc> {
        self.requested_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginAssignmentOperationScheduleOutcome {
    replayed: bool,
}

impl PluginAssignmentOperationScheduleOutcome {
    pub(crate) const fn new(replayed: bool) -> Self {
        Self { replayed }
    }

    pub const fn replayed(self) -> bool {
        self.replayed
    }
}

#[async_trait]
pub trait IPluginAssignmentOperationScheduler: Send + Sync {
    async fn schedule(
        &self,
        request: PluginAssignmentOperationRequest,
    ) -> Result<PluginAssignmentOperationScheduleOutcome, RepositoryError>;
}
