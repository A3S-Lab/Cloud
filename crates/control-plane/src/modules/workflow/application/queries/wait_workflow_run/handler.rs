use super::{WaitWorkflowRun, WORKFLOW_RUN_WAIT_MAX_TIMEOUT};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::domain::{IWorkflowRunRepository, WorkflowRunRecord};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct WaitWorkflowRunHandler {
    repository: Arc<dyn IWorkflowRunRepository>,
}

impl WaitWorkflowRunHandler {
    pub fn new(repository: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { repository }
    }
}

impl QueryHandler<WaitWorkflowRun> for WaitWorkflowRunHandler {
    fn execute(
        &self,
        query: WaitWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunRecord>>> {
        let repository = Arc::clone(&self.repository);
        Box::pin(async move {
            if query.timeout > WORKFLOW_RUN_WAIT_MAX_TIMEOUT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "WorkflowRun wait timeout cannot exceed {} seconds",
                    WORKFLOW_RUN_WAIT_MAX_TIMEOUT.as_secs()
                ))));
            }
            let deadline = tokio::time::Instant::now() + query.timeout;
            loop {
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
                if record.run.status.is_terminal() || tokio::time::Instant::now() >= deadline {
                    return Ok(Ok(record));
                }
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(POLL_INTERVAL.min(remaining)).await;
            }
        })
    }
}
