use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, WorkflowRunId};
use crate::modules::workflow::application::WorkflowRunMutationResult;
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CancelWorkflowRun {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub reason: Option<String>,
    pub actor_principal_id: PrincipalId,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl Command for CancelWorkflowRun {
    type Output = ApplicationResult<WorkflowRunMutationResult>;
}
