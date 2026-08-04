use crate::modules::agents::domain::{AgentExecution, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentConversationId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListAgentExecutions {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub limit: usize,
}

impl Query for ListAgentExecutions {
    type Output = ApplicationResult<Vec<AgentExecution>>;
}

pub struct ListAgentExecutionsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl ListAgentExecutionsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<ListAgentExecutions> for ListAgentExecutionsHandler {
    fn execute(
        &self,
        query: ListAgentExecutions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<AgentExecution>>>>
    {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 200 {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent execution limit must be between 1 and 200".into(),
                )));
            }
            match agents
                .find_conversation(query.organization_id, query.conversation_id)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent conversation not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            Ok(agents
                .list_executions(query.organization_id, query.conversation_id, query.limit)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
