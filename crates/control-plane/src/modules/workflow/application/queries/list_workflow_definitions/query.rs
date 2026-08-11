use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::WorkflowDefinition;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListWorkflowDefinitions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
}

impl Query for ListWorkflowDefinitions {
    type Output = ApplicationResult<Vec<WorkflowDefinition>>;
}
