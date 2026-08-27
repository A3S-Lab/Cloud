use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::application::support::load_checkpoint_snapshot;
use crate::modules::agents::domain::{
    AgentExecutionCheckpointSnapshot, IAgentExecutionCheckpointObjectStore, IAgentRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    AgentExecutionCheckpointId, AgentExecutionId, OrganizationId, RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecutionCheckpointSnapshot {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub checkpoint_id: AgentExecutionCheckpointId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentExecutionCheckpointSnapshot {
    type Output = ApplicationResult<AgentExecutionCheckpointSnapshot>;
}

pub struct GetAgentExecutionCheckpointSnapshotHandler {
    agents: Arc<dyn IAgentRepository>,
    objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
}

impl GetAgentExecutionCheckpointSnapshotHandler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        objects: Arc<dyn IAgentExecutionCheckpointObjectStore>,
    ) -> Self {
        Self { agents, objects }
    }
}

impl QueryHandler<GetAgentExecutionCheckpointSnapshot>
    for GetAgentExecutionCheckpointSnapshotHandler
{
    fn execute(
        &self,
        query: GetAgentExecutionCheckpointSnapshot,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AgentExecutionCheckpointSnapshot>>,
    > {
        let agents = Arc::clone(&self.agents);
        let objects = Arc::clone(&self.objects);
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
            let checkpoint = match agents
                .find_execution_checkpoint(query.organization_id, query.checkpoint_id)
                .await
            {
                Ok(Some(checkpoint))
                    if checkpoint.execution_id == access.execution.id
                        && checkpoint.conversation_id == access.conversation.id =>
                {
                    checkpoint
                }
                Ok(Some(_)) | Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "Agent execution checkpoint not found".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            };
            Ok(load_checkpoint_snapshot(objects, &checkpoint).await)
        })
    }
}
