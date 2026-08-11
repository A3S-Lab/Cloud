use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::application::WorkflowRunView;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListWorkflowRuns {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

impl Query for ListWorkflowRuns {
    type Output = ApplicationResult<Vec<WorkflowRunView>>;
}
