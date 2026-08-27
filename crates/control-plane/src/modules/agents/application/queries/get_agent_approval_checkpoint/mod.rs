use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentApprovalCheckpoint, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentApprovalCheckpointId, AgentExecutionId, OrganizationId, RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentApprovalCheckpoint {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentApprovalCheckpointId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentApprovalCheckpoint {
    type Output = ApplicationResult<AgentApprovalCheckpoint>;
}

pub struct GetAgentApprovalCheckpointHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl GetAgentApprovalCheckpointHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<GetAgentApprovalCheckpoint> for GetAgentApprovalCheckpointHandler {
    fn execute(
        &self,
        query: GetAgentApprovalCheckpoint,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentApprovalCheckpoint>>>
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
                .find_checkpoint(query.organization_id, query.checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id =>
                {
                    Ok(Ok(checkpoint))
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => Ok(Err(
                    ApplicationError::NotFound("Agent approval checkpoint not found".into()),
                )),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
