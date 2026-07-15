use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::edge::domain::Route;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListRoutes {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListRoutes {
    type Output = ApplicationResult<Vec<Route>>;
}

pub struct ListRoutesHandler {
    routes: Arc<dyn IEdgeRepository>,
}

impl ListRoutesHandler {
    pub fn new(routes: Arc<dyn IEdgeRepository>) -> Self {
        Self { routes }
    }
}

impl QueryHandler<ListRoutes> for ListRoutesHandler {
    fn execute(
        &self,
        query: ListRoutes,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Route>>>> {
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            Ok(routes
                .list_routes(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
                .map_err(ApplicationError::from))
        })
    }
}
