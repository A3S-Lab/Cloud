use crate::modules::agents::domain::{AgentExecutionChangeSet, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionChangeSet {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
}

impl Query for GetAgentExecutionChangeSet {
    type Output = ApplicationResult<AgentExecutionChangeSet>;
}

pub struct GetAgentExecutionChangeSetHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentExecutionChangeSetHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
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
        Box::pin(async move {
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
