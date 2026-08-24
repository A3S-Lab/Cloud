use super::GetWorkflowRunDiagnostics;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{
    IWorkflowRunDiagnosticsReader, IWorkflowRunRepository, WorkflowRunDiagnostics,
};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunDiagnosticsHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
    diagnostics: Arc<dyn IWorkflowRunDiagnosticsReader>,
}

impl GetWorkflowRunDiagnosticsHandler {
    pub fn new(
        repository: Arc<dyn IWorkflowRunRepository>,
        diagnostics: Arc<dyn IWorkflowRunDiagnosticsReader>,
    ) -> Self {
        Self {
            repository,
            diagnostics,
        }
    }
}

impl QueryHandler<GetWorkflowRunDiagnostics> for GetWorkflowRunDiagnosticsHandler {
    fn execute(
        &self,
        query: GetWorkflowRunDiagnostics,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunDiagnostics>>>
    {
        let repository = Arc::clone(&self.repository);
        let diagnostics = Arc::clone(&self.diagnostics);
        Box::pin(async move {
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
            Ok(diagnostics
                .inspect(&record)
                .await
                .map_err(ApplicationError::Unavailable))
        })
    }
}
