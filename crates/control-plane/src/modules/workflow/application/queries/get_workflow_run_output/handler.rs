use super::{GetWorkflowRunOutput, WorkflowRunOutput};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::domain::{IWorkflowRunRepository, WorkflowRunStatus};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetWorkflowRunOutputHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
}

impl GetWorkflowRunOutputHandler {
    pub fn new(repository: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<GetWorkflowRunOutput> for GetWorkflowRunOutputHandler {
    fn execute(
        &self,
        query: GetWorkflowRunOutput,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunOutput>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            let record = match repository
                .find(query.organization_id, query.workflow_run_id)
                .await
            {
                Ok(Some(record)) => record,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "WorkflowRun not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if record.run.status != WorkflowRunStatus::Completed {
                return Ok(Err(ApplicationError::Conflict(format!(
                    "WorkflowRun output is unavailable while status is {}",
                    record.run.status.as_str()
                ))));
            }
            let output = record.run.output.ok_or_else(|| {
                a3s_boot::BootError::Internal(
                    "completed WorkflowRun lost its validated output".into(),
                )
            })?;
            let output_digest = record.run.output_digest.ok_or_else(|| {
                a3s_boot::BootError::Internal(
                    "completed WorkflowRun lost its validated output digest".into(),
                )
            })?;
            let finished_at = record.run.finished_at.ok_or_else(|| {
                a3s_boot::BootError::Internal(
                    "completed WorkflowRun lost its validated finish time".into(),
                )
            })?;
            Ok(Ok(WorkflowRunOutput {
                workflow_run_id: record.run.id,
                output,
                output_digest,
                finished_at,
            }))
        })
    }
}
