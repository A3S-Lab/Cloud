use crate::modules::executions::domain::Execution;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{ExecutionId, OrganizationId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CancelExecution {
    pub organization_id: OrganizationId,
    pub execution_id: ExecutionId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelExecution {
    type Output = ApplicationResult<CancelExecutionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelExecutionResult {
    pub execution: Execution,
    pub replayed: bool,
}
