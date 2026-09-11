use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowDefinitionId};
use crate::modules::workflow::domain::WorkflowRevision;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct ListWorkflowRevisions {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub access: WorkflowAccess,
}

impl Query for ListWorkflowRevisions {
    type Output = ApplicationResult<Vec<WorkflowRevision>>;
}
