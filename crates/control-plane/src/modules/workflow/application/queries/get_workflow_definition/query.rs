use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowDefinitionId};
use crate::modules::workflow::domain::WorkflowDefinition;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowDefinition {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetWorkflowDefinition {
    type Output = ApplicationResult<WorkflowDefinition>;
}
