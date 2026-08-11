use crate::modules::edge::application::McpRoutePolicyApplicationService;
use crate::modules::edge::domain::McpRoutePolicy;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetMcpRoutePolicy {
    pub organization_id: OrganizationId,
    pub route_id: RouteId,
}

impl Query for GetMcpRoutePolicy {
    type Output = ApplicationResult<McpRoutePolicy>;
}

pub struct GetMcpRoutePolicyHandler {
    service: Arc<McpRoutePolicyApplicationService>,
}

impl GetMcpRoutePolicyHandler {
    pub fn new(service: Arc<McpRoutePolicyApplicationService>) -> Self {
        Self { service }
    }
}

impl QueryHandler<GetMcpRoutePolicy> for GetMcpRoutePolicyHandler {
    fn execute(
        &self,
        query: GetMcpRoutePolicy,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<McpRoutePolicy>>> {
        let service = Arc::clone(&self.service);
        Box::pin(async move { Ok(service.get(query.organization_id, query.route_id).await) })
    }
}
