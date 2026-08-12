use crate::modules::agents::application::resource_access::AgentResourceAccess;
use crate::modules::agents::domain::{AgentExecution, IAgentRepository};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{AgentExecutionId, OrganizationId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetAgentExecution {
    pub organization_id: OrganizationId,
    pub execution_id: AgentExecutionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetAgentExecution {
    type Output = ApplicationResult<AgentExecution>;
}

pub struct GetAgentExecutionHandler {
    resource_access: AgentResourceAccess,
}

impl GetAgentExecutionHandler {
    pub fn new(agents: Arc<dyn IAgentRepository>) -> Self {
        Self {
            resource_access: AgentResourceAccess::new(agents),
        }
    }
}

impl QueryHandler<GetAgentExecution> for GetAgentExecutionHandler {
    fn execute(
        &self,
        query: GetAgentExecution,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AgentExecution>>> {
        let resource_access = self.resource_access.clone();
        Box::pin(async move {
            Ok(resource_access
                .execution(
                    query.organization_id,
                    query.execution_id,
                    &query.resource_access,
                )
                .await
                .map(|access| access.execution))
        })
    }
}
