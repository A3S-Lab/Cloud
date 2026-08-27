use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{
    AgentApprovalCheckpoint, AgentApprovalCheckpointStatus, IAgentRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListAgentApprovalCheckpoints {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
    pub status: Option<AgentApprovalCheckpointStatus>,
    pub limit: usize,
}

impl Query for ListAgentApprovalCheckpoints {
    type Output = ApplicationResult<Vec<AgentApprovalCheckpoint>>;
}

pub struct ListAgentApprovalCheckpointsHandler {
    agents: Arc<dyn IAgentRepository>,
}

impl ListAgentApprovalCheckpointsHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self { agents }
    }
}

impl QueryHandler<ListAgentApprovalCheckpoints> for ListAgentApprovalCheckpointsHandler {
    fn execute(
        &self,
        query: ListAgentApprovalCheckpoints,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AgentApprovalCheckpoint>>>,
    > {
        let agents = Arc::clone(&self.agents);
        Box::pin(async move {
            if query.limit == 0 || query.limit > 1_000 {
                return Ok(Err(ApplicationError::Invalid(
                    "Agent approval checkpoint limit must be between 1 and 1000".into(),
                )));
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
                .list_checkpoints(
                    query.organization_id,
                    query.execution_id,
                    query.status,
                    query.limit,
                )
                .await
            {
                Ok(checkpoints) => Ok(Ok(checkpoints)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
