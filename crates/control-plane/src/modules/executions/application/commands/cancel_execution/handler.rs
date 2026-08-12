use super::{CancelExecution, CancelExecutionResult};
use crate::modules::executions::application::execution_cancellation::{
    ExecutionCancellation, ExecutionCancellationService,
};
use crate::modules::executions::application::resource_access::ExecutionResourceAccess;
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CancelExecutionHandler {
    executions: Arc<dyn IExecutionRepository>,
}

impl CancelExecutionHandler {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }
}

impl CommandHandler<CancelExecution> for CancelExecutionHandler {
    fn execute(
        &self,
        command: CancelExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CancelExecutionResult>>>
    {
        let executions = Arc::clone(&self.executions);
        Box::pin(async move {
            let execution = match ExecutionResourceAccess::new(Arc::clone(&executions))
                .execution(
                    command.organization_id,
                    command.execution_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(execution) => execution,
                Err(error) => return Ok(Err(error)),
            };
            Ok(ExecutionCancellationService::new(executions)
                .cancel(ExecutionCancellation {
                    execution,
                    idempotency_key: command.idempotency_key,
                    request_id: command.request_id,
                    requested_at: command.requested_at,
                })
                .await)
        })
    }
}
