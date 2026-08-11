use super::GetWorkflowRun;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::WorkflowRunView;
use crate::modules::workflow::domain::IWorkflowRunRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunHandler {
    runs: Arc<dyn IWorkflowRunRepository>,
    operations: Arc<dyn IOperationRepository>,
}

impl GetWorkflowRunHandler {
    pub fn new(
        runs: Arc<dyn IWorkflowRunRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self { runs, operations }
    }
}

impl QueryHandler<GetWorkflowRun> for GetWorkflowRunHandler {
    fn execute(
        &self,
        query: GetWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunView>>> {
        let runs = Arc::clone(&self.runs);
        let operations = Arc::clone(&self.operations);
        Box::pin(async move {
            let run = match runs
                .find(query.organization_id, query.workflow_run_id)
                .await
            {
                Ok(Some(value)) => value,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "WorkflowRun not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(
                super::super::workflow_run_view::load(run, operations.as_ref())
                    .await
                    .map_err(Into::into),
            )
        })
    }
}
