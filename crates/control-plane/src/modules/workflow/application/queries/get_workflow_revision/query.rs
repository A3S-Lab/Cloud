use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OrganizationId, WorkflowDefinitionId, WorkflowRevisionId,
};
use crate::modules::workflow::domain::WorkflowRevision;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowRevision {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub workflow_revision_id: WorkflowRevisionId,
}

impl Query for GetWorkflowRevision {
    type Output = ApplicationResult<WorkflowRevision>;
}
