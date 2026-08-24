use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowRunId};
use crate::modules::workflow::domain::WorkflowRunDiagnostics;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowRunDiagnostics {
    pub organization_id: OrganizationId,
    pub workflow_run_id: WorkflowRunId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetWorkflowRunDiagnostics {
    type Output = ApplicationResult<WorkflowRunDiagnostics>;
}
