use crate::modules::agents::domain::{AgentConversation, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListAgentConversations {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub limit: usize,
}

impl Query for ListAgentConversations {
    type Output = ApplicationResult<Vec<AgentConversation>>;
}

pub struct ListAgentConversationsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl ListAgentConversationsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<ListAgentConversations> for ListAgentConversationsHandler {
    fn execute(
        &self,
        query: ListAgentConversations,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<AgentConversation>>>>
    {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 200 {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent conversation limit must be between 1 and 200".into(),
                )));
            }
            Ok(agents
                .list_conversations(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.limit,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
