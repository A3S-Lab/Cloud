use super::{CancelAgentExecution, CancelAgentExecutionResult};
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
                        && execution.id == command.execution_id =>
                {
                    let conversation = match agents
                        .find_conversation(execution.organization_id, execution.conversation_id)
                        .await
                    {
                        Ok(Some(conversation)) => conversation,
                        Ok(None) => {
                            return Err(BootError::Internal(
                                "replayed Agent cancellation lost its conversation".into(),
                            ));
                        }
                        Err(error) => return Ok(Err(error.into())),
                    };
                    return Ok(Ok(CancelAgentExecutionResult {
                        conversation,
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

            let mut execution = match agents
                .find_execution(command.organization_id, command.execution_id)
                .await
            {
                Ok(Some(execution)) => execution,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent execution not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
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
