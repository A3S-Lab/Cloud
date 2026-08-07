use crate::modules::edge::application::McpRoutePolicyApplicationService;
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListMcpRoutePolicies {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListMcpRoutePolicies {
    type Output = ApplicationResult<Vec<McpRoutePolicy>>;
}

pub struct ListMcpRoutePoliciesHandler {
    service: Arc<McpRoutePolicyApplicationService>,
}

impl ListMcpRoutePoliciesHandler {
    pub fn new(service: Arc<McpRoutePolicyApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<ListMcpRoutePolicies> for ListMcpRoutePoliciesHandler {
    fn execute(
        &self,
        query: ListMcpRoutePolicies,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<McpRoutePolicy>>>>
    {
        let service = Arc::clone(&self.service);
        Box::pin(async move {
            Ok(service
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await)
        })
    }
}
