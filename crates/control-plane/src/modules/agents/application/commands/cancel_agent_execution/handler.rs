use super::{CancelAgentExecution, CancelAgentExecutionResult};
use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::support::{idempotency, validate_request_id};
use crate::modules::agents::domain::{
    AgentExecutionCancellationRequested, IAgentRepository, RequestAgentExecutionCancellationWrite,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CancelAgentExecutionHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl CancelAgentExecutionHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl CommandHandler<CancelAgentExecution> for CancelAgentExecutionHandler {
    fn execute(
        &self,
        command: CancelAgentExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CancelAgentExecutionResult>>>
    {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if let Err(error) = validate_request_id(command.request_id) {
                return Ok(Err(error));
            }
            let access = match AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    command.organization_id,
                    command.execution_id,
                    &command.resource_access,
                )
                .await
            {
                Ok(access) => access,
                Err(error) => return Ok(Err(error)),
            };
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-executions/{}/cancel",
                    command.organization_id, command.execution_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "executionId": command.execution_id,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents.replay_execution(&idempotency).await {
                Ok(Some(execution))
                    if execution.organization_id == command.organization_id
                        && execution.id == command.execution_id
                        && execution.conversation_id == access.execution.conversation_id =>
                {
                    return Ok(Ok(CancelAgentExecutionResult {
                        conversation: access.conversation,
                        execution,
                        replayed: true,
                    }));
                }
                Ok(Some(_)) => {
                    return Err(BootError::Internal(
                        "Agent cancellation replay changed its immutable identity".into(),
                    ));
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let mut execution = access.execution;
            let expected_version = execution.aggregate_version;
            if let Err(error) = execution.request_cancellation(command.requested_at) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event =
                AgentExecutionCancellationRequested::envelope(&execution, command.request_id)
                    .map_err(|error| BootError::Internal(error.to_string()))?;
            match agents
                .request_cancellation(RequestAgentExecutionCancellationWrite {
                    execution,
                    expected_version,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(CancelAgentExecutionResult {
                    conversation: write.conversation,
                    execution: write.execution,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
