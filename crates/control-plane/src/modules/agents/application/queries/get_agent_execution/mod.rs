use crate::modules::agents::domain::{AgentExecution, IAgentRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecution {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
}

impl Query for GetAgentExecution {
    type Output = ApplicationResult<AgentExecution>;
}

pub struct GetAgentExecutionHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentExecutionHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentExecution> for GetAgentExecutionHandler {
    fn execute(
        &self,
        query: GetAgentExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentExecution>>> {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            match agents
                .find_execution(query.organization_id, query.execution_id)
                .await
            {
                Ok(Some(execution)) => Ok(Ok(execution)),
                Ok(None) => Ok(Err(ApplicationError::NotFound(
                    "Agent execution not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
