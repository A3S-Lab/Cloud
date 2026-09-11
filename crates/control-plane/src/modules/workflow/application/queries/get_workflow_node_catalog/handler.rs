use super::GetWorkflowNodeCatalog;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workflow::application::{IWorkflowProjectAccess, WorkflowProjectScope};
use crate::modules::workflow::domain::WorkflowNodeCatalog;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowNodeCatalogHandler {
    projects: Arc<dyn IWorkflowProjectAccess>,
}

impl GetWorkflowNodeCatalogHandler {
    pub fn new(projects: Arc<dyn IWorkflowProjectAccess>) -> Self {
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
            let scope = match WorkflowProjectScope::new(query.organization_id, query.project_id) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match projects.project_exists(scope).await {
                Ok(true) => {}
                Ok(false) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound("project not found".into())))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if !query.access.project_is_visible(query.project_id) {
                return Ok(Err(ApplicationError::NotFound("project not found".into())));
            }
            Ok(WorkflowNodeCatalog::checked_in().map_err(ApplicationError::Internal))
        })
    }
}
