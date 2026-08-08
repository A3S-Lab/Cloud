use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowDefinitionId};
use crate::modules::workflow::domain::WorkflowDefinition;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowDefinition {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
}

impl Query for GetWorkflowDefinition {
    type Output = ApplicationResult<WorkflowDefinition>;
}
