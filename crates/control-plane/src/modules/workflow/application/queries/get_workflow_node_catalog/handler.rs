use super::GetWorkflowNodeCatalog;
use crate::modules::projects::application::resource_access::ProjectResourceAccess;
use crate::modules::projects::domain::repositories::IProjectRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::WorkflowNodeCatalog;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowNodeCatalogHandler {
    projects: Arc<dyn IProjectRepository>,
}

impl GetWorkflowNodeCatalogHandler {
    pub fn new(projects: Arc<dyn IProjectRepository>) -> Self {
        Self { projects }
    }
}

impl QueryHandler<GetWorkflowNodeCatalog> for GetWorkflowNodeCatalogHandler {
    fn execute(
        &self,
        query: GetWorkflowNodeCatalog,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowNodeCatalog>>>
    {
        let projects = Arc::clone(&self.projects);
        Box::pin(async move {
            if let Err(error) = ProjectResourceAccess::new(projects)
                .project(
                    query.organization_id,
                    query.project_id,
                    &query.resource_access,
                )
                .await
            {
                return Ok(Err(error));
            }
            Ok(WorkflowNodeCatalog::checked_in().map_err(ApplicationError::Internal))
        })
    }
}
