use crate::modules::agents::domain::{AgentExecutionEvent, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentConversationId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionEvents {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub after_sequence: Option<u64>,
    pub limit: usize,
}

impl Query for GetAgentExecutionEvents {
    type Output = ApplicationResult<AgentExecutionEventPage>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentExecutionEventPage {
    pub conversation_id: AgentConversationId,
    pub head_sequence: u64,
    pub records: Vec<AgentExecutionEvent>,
    pub next_after_sequence: Option<u64>,
}

pub struct GetAgentExecutionEventsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentExecutionEventsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentExecutionEvents> for GetAgentExecutionEventsHandler {
    fn execute(
        &self,
        query: GetAgentExecutionEvents,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentExecutionEventPage>>>
    {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 200 {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent event limit must be between 1 and 200".into(),
                )));
            }
            let conversation = match agents
                .find_conversation(query.organization_id, query.conversation_id)
                .await
            {
                Ok(Some(conversation)) => conversation,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent conversation not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            if query
                .after_sequence
                .is_some_and(|sequence| sequence > conversation.last_event_sequence)
            {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent event cursor is ahead of the conversation head".into(),
                )));
            }
            let records = match agents
                .list_events(
                    query.organization_id,
                    query.conversation_id,
                    query.after_sequence,
                    query.limit,
                )
                .await
            {
                Ok(records) => records,
                Err(error) => return Ok(Err(error.into())),
            };
            let next_after_sequence = records.last().map(|event| event.sequence);
            Ok(Ok(AgentExecutionEventPage {
                conversation_id: conversation.id,
                head_sequence: conversation.last_event_sequence,
                records,
                next_after_sequence,
            }))
        })
    }
}
