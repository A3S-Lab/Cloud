use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowDefinitionId};
use crate::modules::workflow::domain::WorkflowDefinition;
use a3s_boot::Query;
use crate::modules::workflow::application::WorkflowAccess;

#[derive(Debug, Clone)]
pub struct GetWorkflowDefinition {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub access: WorkflowAccess,
}

impl Query for GetWorkflowDefinition {
    type Output = ApplicationResult<WorkflowDefinition>;
}
