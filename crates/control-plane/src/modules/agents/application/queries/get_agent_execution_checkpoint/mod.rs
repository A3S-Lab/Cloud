use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentExecutionCheckpoint, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, OrganizationId, RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionCheckpoint {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentExecutionCheckpointId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentExecutionCheckpoint {
    type Output = ApplicationResult<AgentExecutionCheckpoint>;
}

pub struct GetAgentExecutionCheckpointHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentExecutionCheckpointHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentExecutionCheckpoint> for GetAgentExecutionCheckpointHandler {
    fn execute(
        &self,
        query: GetAgentExecutionCheckpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentExecutionCheckpoint>>>
    {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            let access = match AgentResourceAccess::new(Arc::clone(&agents))
                .execution(
                    query.organization_id,
                    query.execution_id,
                    &query.resource_access,
                )
                .await
            {
                Ok(access) => access,
                Err(error) => return Ok(Err(error)),
            };
            match agents
                .find_execution_checkpoint(query.organization_id, query.checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id =>
                {
                    Ok(Ok(checkpoint))
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => Ok(Err(
                    ApplicationError::NotFound("Agent execution checkpoint not found".into()),
                )),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
