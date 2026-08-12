use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentExecutionChangeSet, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionChangeSet {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentExecutionChangeSet {
    type Output = ApplicationResult<AgentExecutionChangeSet>;
}

pub struct GetAgentExecutionChangeSetHandler {
    agents: Arc<dyn IAgentRepository>,
    resource_access: AgentResourceAccess,
}

impl GetAgentExecutionChangeSetHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self {
            agents: Arc::clone(&agents),
            resource_access: AgentResourceAccess::new(agents),
        }
    }
}

impl QueryHandler<GetAgentExecutionChangeSet> for GetAgentExecutionChangeSetHandler {
    fn execute(
        &self,
        query: GetAgentExecutionChangeSet,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentExecutionChangeSet>>>
    {
        let agents = Arc::clone(&self.agents);
        let resource_access = self.resource_access.clone();
        Box::pin(async move {
            if let Err(error) = resource_access
                .execution(
                    query.organization_id,
                    query.execution_id,
                    &query.resource_access,
                )
                .await
            {
                return Ok(Err(error));
            }
            match agents
                .find_execution_change_set(query.organization_id, query.execution_id)
                .await
            {
                Ok(Some(change_set)) => Ok(Ok(change_set)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Agent execution change set not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
