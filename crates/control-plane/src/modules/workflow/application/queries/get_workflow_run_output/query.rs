use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, Sha256Digest, WorkflowRunId};
use a3s_boot::Query;
use chrono::{DateTime, Utc};
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunOutput {
    pub workflow_run_id: WorkflowRunId,
    pub output: serde_json::Value,
    pub output_digest: Sha256Digest,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GetWorkflowRunOutput {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowRunOutput {
    type Output = ApplicationResult<WorkflowRunOutput>;
}
