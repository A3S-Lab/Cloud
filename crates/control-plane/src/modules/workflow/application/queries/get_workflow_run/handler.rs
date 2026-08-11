use super::GetWorkflowRun;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowRunRepository, WorkflowRunRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
}

impl GetWorkflowRunHandler {
    pub fn new(repository: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetWorkflowRun> for GetWorkflowRunHandler {
    fn execute(
        &self,
        query: GetWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunRecord>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            match repository
                .find(query.organization_id, query.workflow_run_id)
                .await
            {
                Ok(Some(record)) => Ok(Ok(record)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "WorkflowRun not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
