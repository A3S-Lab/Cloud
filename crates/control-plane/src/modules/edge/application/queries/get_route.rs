use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::Route;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{OrganizationId, RouteId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct GetRoute {
    pub organization_id: OrganizationId,
    pub route_id: RouteId,
}

impl Query for GetRoute {
    type Output = ApplicationResult<Route>;
}

pub struct GetRouteHandler {
    routes: Arc<dyn IEdgeRepository>,
}

impl GetRouteHandler {
    pub fn new(routes: Arc<dyn IEdgeRepository>) -> Self {
        Self { routes }
    }
}

impl QueryHandler<GetRoute> for GetRouteHandler {
    fn execute(
        &self,
        query: GetRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Route>>> {
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            Ok(routes
                .find_route(query.organization_id, query.route_id)
                .await
                .map_err(ApplicationError::from))
        })
    }
}
