use super::ListWorkflowRuns;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::workflow::application::WorkflowRunView;
use crate::modules::workflow::domain::IWorkflowRunRepository;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct ListWorkflowRunsHandler {
    runs: Arc<dyn IWorkflowRunRepository>,
    operations: Arc<dyn IOperationRepository>,
}

impl ListWorkflowRunsHandler {
    pub fn new(
        runs: Arc<dyn IWorkflowRunRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self { runs, operations }
    }
}

impl QueryHandler<ListWorkflowRuns> for ListWorkflowRunsHandler {
    fn execute(
        &self,
        query: ListWorkflowRuns,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<WorkflowRunView>>>>
    {
        let runs = Arc::clone(&self.runs);
        let operations = Arc::clone(&self.operations);
        Box::pin(async move {
            let runs = match runs.list(query.organization_id, query.project_id).await {
                Ok(values) => values,
                Err(error) => return Ok(Err(error.into())),
            };
            let mut values = Vec::with_capacity(runs.len());
            for run in runs {
                match super::super::workflow_run_view::load(run, operations.as_ref()).await {
                    Ok(value) => values.push(value),
                    Err(error) => return Ok(Err(error.into())),
                }
            }
            Ok(Ok(values))
        })
    }
}
