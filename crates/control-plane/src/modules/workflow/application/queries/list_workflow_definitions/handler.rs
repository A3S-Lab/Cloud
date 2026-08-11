use super::ListWorkflowDefinitions;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::domain::{IWorkflowDefinitionRepository, WorkflowDefinition};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowDefinitionsHandler {
    repository: Arc<dyn IWorkflowDefinitionRepository>,
}

impl ListWorkflowDefinitionsHandler {
    pub fn new(repository: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListWorkflowDefinitions> for ListWorkflowDefinitionsHandler {
    fn execute(
        &self,
        query: ListWorkflowDefinitions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowDefinition>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list(query.organization_id, query.project_id)
                .await
                .map_err(Into::into))
        })
    }
}
