use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId};
use crate::modules::workflow::domain::WorkflowNodeCatalog;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetWorkflowNodeCatalog {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetWorkflowNodeCatalog {
    type Output = ApplicationResult<WorkflowNodeCatalog>;
}
