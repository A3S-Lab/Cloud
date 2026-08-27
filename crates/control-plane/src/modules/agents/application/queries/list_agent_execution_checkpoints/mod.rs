use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentExecutionCheckpoint, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const MAX_AGENT_EXECUTION_CHECKPOINT_LIST_LIMIT: usize = 1_000;

#[derive(Debug, Clone)]
pub struct ListAgentExecutionCheckpoints {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
    pub limit: usize,
}

impl Query for ListAgentExecutionCheckpoints {
    type Output = ApplicationResult<Vec<AgentExecutionCheckpoint>>;
}

pub struct ListAgentExecutionCheckpointsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl ListAgentExecutionCheckpointsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<ListAgentExecutionCheckpoints> for ListAgentExecutionCheckpointsHandler {
    fn execute(
        &self,
        query: ListAgentExecutionCheckpoints,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AgentExecutionCheckpoint>>>,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > MAX_AGENT_EXECUTION_CHECKPOINT_LIST_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Agent execution checkpoint limit must be between 1 and {MAX_AGENT_EXECUTION_CHECKPOINT_LIST_LIMIT}"
                ))));
            }
            if let Err(error) = AgentResourceAccess::new(Arc::clone(&agents))
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
                .list_execution_checkpoints(query.organization_id, query.execution_id, query.limit)
                .await
            {
                Ok(checkpoints) => Ok(Ok(checkpoints)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
