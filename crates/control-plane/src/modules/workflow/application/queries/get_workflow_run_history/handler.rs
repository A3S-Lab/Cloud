use super::{GetWorkflowRunHistory, WORKFLOW_RUN_HISTORY_MAX_LIMIT};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{
    IWorkflowRunHistoryReader, IWorkflowRunRepository, WorkflowRunHistoryPage,
};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunHistoryHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
    history: Arc<dyn IWorkflowRunHistoryReader>,
}

impl GetWorkflowRunHistoryHandler {
    pub fn new(
        repository: Arc<dyn IWorkflowRunRepository>,
        history: Arc<dyn IWorkflowRunHistoryReader>,
    ) -> Self {
        Self {
            repository,
            history,
        }
    }
}

impl QueryHandler<GetWorkflowRunHistory> for GetWorkflowRunHistoryHandler {
    fn execute(
        &self,
        query: GetWorkflowRunHistory,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunHistoryPage>>>
    {
        let repository = Arc::clone(&self.repository);
        let history = Arc::clone(&self.history);
        Box::pin(async move {
            if query.limit == 0 || query.limit > WORKFLOW_RUN_HISTORY_MAX_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "WorkflowRun history limit must be between 1 and {WORKFLOW_RUN_HISTORY_MAX_LIMIT}"
                ))));
            }
            let record = match resource_access::workflow_run(
                repository.as_ref(),
                query.organization_id,
                query.workflow_run_id,
                &query.resource_access,
            )
            .await
            {
                Ok(record) => record,
                Err(error) => return Ok(Err(error)),
            };
            Ok(history
                .read(&record.run.flow_run_id, query.after_sequence, query.limit)
                .await
                .map_err(ApplicationError::Unavailable))
        })
    }
}
