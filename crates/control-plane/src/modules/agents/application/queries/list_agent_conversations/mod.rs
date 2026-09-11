use crate::modules::agents::application::resource_access::AgentAccess;
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
    pub access: AgentAccess,
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "agent conversations not found".into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::agents::application::resource_access::{AgentAccess, AgentAccessScope};
    use crate::modules::agents::infrastructure::InMemoryAgentRepository;
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListAgentConversationsHandler::new(Arc::new(InMemoryAgentRepository::new()));
        let result = handler
            .execute(
                ListAgentConversations {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    limit: 50,
                    access: AgentAccess::restricted([AgentAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("handler");
        assert!(matches!(
            result,
            Err(ApplicationError::NotFound(message))
                if message == "agent conversations not found"
        ));
    }
}
