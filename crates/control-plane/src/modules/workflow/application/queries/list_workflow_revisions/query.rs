use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, WorkflowDefinitionId};
use crate::modules::workflow::domain::WorkflowRevision;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListWorkflowRevisions {
    pub organization_id: OrganizationId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListWorkflowRevisions {
    type Output = ApplicationResult<Vec<WorkflowRevision>>;
}
