use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::WorkflowNodeCatalog;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetWorkflowNodeCatalog {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowNodeCatalog {
    type Output = ApplicationResult<WorkflowNodeCatalog>;
}
