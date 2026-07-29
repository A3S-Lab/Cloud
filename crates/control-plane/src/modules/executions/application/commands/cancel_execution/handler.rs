use super::{CancelExecution, CancelExecutionResult};
use crate::modules::executions::domain::events::ExecutionCancellationRequested;
use crate::modules::executions::domain::{IExecutionRepository, TransitionExecution};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
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
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "executionId": command.execution_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/executions/{}/cancellation",
                    command.organization_id, command.execution_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Some(replay) = match executions.replay(&idempotency).await {
                Ok(replay) => replay,
                Err(error) => return Ok(Err(error.into())),
            } {
                return Ok(Ok(CancelExecutionResult {
                    execution: replay,
                    replayed: true,
                }));
            }
            let mut execution = match executions
                .find(command.organization_id, command.execution_id)
                .await
            {
                Ok(Some(execution)) => execution,
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "execution not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let expected_version = execution.aggregate_version;
            if let Err(error) = execution.request_cancellation(command.requested_at) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = ExecutionCancellationRequested::envelope(&execution, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match executions
                .request_cancellation(TransitionExecution {
                    execution,
                    expected_version,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(write) => Ok(Ok(CancelExecutionResult {
                    execution: write.execution,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
