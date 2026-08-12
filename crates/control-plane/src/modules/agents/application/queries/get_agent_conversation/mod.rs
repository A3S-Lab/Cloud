use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentConversation, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AgentConversationId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentConversation {
    pub organization_id: OrganizationId,
    pub conversation_id: AgentConversationId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentConversation {
    type Output = ApplicationResult<AgentConversation>;
}

pub struct GetAgentConversationHandler {
    resource_access: AgentResourceAccess,
}

impl GetAgentConversationHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self {
            resource_access: AgentResourceAccess::new(agents),
        }
    }
}

impl QueryHandler<GetAgentConversation> for GetAgentConversationHandler {
    fn execute(
        &self,
        query: GetAgentConversation,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentConversation>>> {
        let resource_access = self.resource_access.clone();
        Box::pin(async move {
            Ok(resource_access
                .conversation(
                    query.organization_id,
                    query.conversation_id,
                    &query.resource_access,
                )
                .await)
        })
    }
}
