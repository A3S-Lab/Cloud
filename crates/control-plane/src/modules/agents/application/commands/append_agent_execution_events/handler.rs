use super::AppendAgentExecutionEvents;
use crate::modules::agents::application::support::idempotency;
use crate::modules::agents::domain::{
    AppendAgentExecutionEventsWrite, IAgentRepository, MAX_AGENT_EVENTS_PER_APPEND,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct AppendAgentExecutionEventsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl AppendAgentExecutionEventsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl CommandHandler<AppendAgentExecutionEvents> for AppendAgentExecutionEventsHandler {
    fn execute(
        &self,
        command: AppendAgentExecutionEvents,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<
            ApplicationResult<crate::modules::agents::domain::AgentExecutionEventsWrite>,
        >,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if command.events.is_empty() || command.events.len() > MAX_AGENT_EVENTS_PER_APPEND {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Agent event batch must contain between 1 and {MAX_AGENT_EVENTS_PER_APPEND} events"
                ))));
            }
            for event in &command.events {
                if let Err(error) = event.content.validate() {
                    return Ok(Err(ApplicationError::Invalid(error)));
                }
            }
            let digest_input = command
                .events
                .iter()
                .map(|event| {
                    serde_json::json!({
                        "kind": event.kind.as_str(),
                        "contentDigest": event.content.digest().as_str(),
                        "contentSizeBytes": event.content.size_bytes(),
                        "occurredAt": event.occurred_at,
                    })
                })
                .collect::<Vec<_>>();
            let idempotency = match idempotency(
                format!(
                    "organizations/{}/agent-conversations/{}/executions/{}/events",
                    command.organization_id, command.conversation_id, command.execution_id
                ),
                command.idempotency_key,
                &serde_json::json!({
                    "organizationId": command.organization_id,
                    "conversationId": command.conversation_id,
                    "executionId": command.execution_id,
                    "events": digest_input,
                }),
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(error)),
            };
            match agents
                .append_events(AppendAgentExecutionEventsWrite {
                    organization_id: command.organization_id,
                    conversation_id: command.conversation_id,
                    execution_id: command.execution_id,
                    events: command.events,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(write)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
