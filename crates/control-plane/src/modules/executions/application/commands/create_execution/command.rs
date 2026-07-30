use crate::modules::executions::domain::{Execution, ExecutionTemplate};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CreateExecutionCommand {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub template: ExecutionTemplate,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CreateExecutionCommand {
    type Output = ApplicationResult<CreateExecutionResult>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateExecutionResult {
    pub execution: Execution,
    pub replayed: bool,
}
