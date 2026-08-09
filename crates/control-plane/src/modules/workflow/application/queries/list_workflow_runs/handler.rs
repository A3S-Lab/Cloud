use super::{ListWorkflowRuns, WORKFLOW_RUN_LIST_MAX_LIMIT};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowRunRepository, WorkflowRunRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowRunsHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
}

impl ListWorkflowRunsHandler {
    pub fn new(repository: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<ListWorkflowRuns> for ListWorkflowRunsHandler {
    fn execute(
        &self,
        query: ListWorkflowRuns,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowRunRecord>>>>
    {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if query.limit == 0 || query.limit > WORKFLOW_RUN_LIST_MAX_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "WorkflowRun limit must be between 1 and {WORKFLOW_RUN_LIST_MAX_LIMIT}"
                ))));
            }
            Ok(repository
                .list(query.organization_id, query.project_id, query.limit)
                .await
                .map_err(Into::into))
        })
    }
}
