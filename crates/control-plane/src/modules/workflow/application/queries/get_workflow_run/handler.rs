use super::GetWorkflowRun;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::resource_access;
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
            Ok(resource_access::workflow_run(
                repository.as_ref(),
                query.organization_id,
                query.workflow_run_id,
                &query.resource_access,
            )
            .await)
        })
    }
}
