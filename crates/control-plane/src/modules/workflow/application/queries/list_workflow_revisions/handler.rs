use super::ListWorkflowRevisions;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::domain::{IWorkflowDefinitionRepository, WorkflowRevision};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowRevisionsHandler {
    repository: Arc<dyn IWorkflowDefinitionRepository>,
}

impl ListWorkflowRevisionsHandler {
    pub fn new(repository: Arc<dyn IWorkflowDefinitionRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListWorkflowRevisions> for ListWorkflowRevisionsHandler {
    fn execute(
        &self,
        query: ListWorkflowRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowRevision>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            Ok(repository
                .list_revisions(query.organization_id, query.workflow_definition_id)
                .await
                .map_err(Into::into))
        })
    }
}
