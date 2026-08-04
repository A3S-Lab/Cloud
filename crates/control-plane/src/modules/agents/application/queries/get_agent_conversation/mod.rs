use crate::modules::agents::domain::{AgentConversation, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentConversationId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentConversation {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
}

impl Query for GetAgentConversation {
    type Output = ApplicationResult<AgentConversation>;
}

pub struct GetAgentConversationHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentConversationHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentConversation> for GetAgentConversationHandler {
    fn execute(
        &self,
        query: GetAgentConversation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentConversation>>> {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            match agents
                .find_conversation(query.organization_id, query.conversation_id)
                .await
            {
                Ok(Some(conversation)) => Ok(Ok(conversation)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Agent conversation not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
