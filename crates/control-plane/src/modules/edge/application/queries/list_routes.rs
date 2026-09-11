use crate::modules::edge::application::resource_access::EdgeAccess;
use crate::modules::edge::domain::Route;
use crate::modules::edge::domain::repositories::IEdgeRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ListRoutes {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub access: EdgeAccess,
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
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound("routes not found".into())));
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::edge::InMemoryEdgeRepository;
    use crate::modules::edge::application::resource_access::EdgeAccessScope;
    use a3s_boot::ModuleRef;

    #[tokio::test]
    async fn restricted_query_fails_closed_before_listing_an_ungranted_environment() {
        let project_id = ProjectId::new();
        let handler = ListRoutesHandler::new(Arc::new(InMemoryEdgeRepository::new()));
        let result = handler
            .execute(
                ListRoutes {
                    organization_id: OrganizationId::new(),
                    project_id,
                    environment_id: EnvironmentId::new(),
                    access: EdgeAccess::restricted([EdgeAccessScope::Project {
                        project_id: ProjectId::new(),
                    }]),
                },
                CqrsContext::new(ModuleRef::new()),
            )
            .await
            .expect("execute list query");

        assert_eq!(
            result,
            Err(ApplicationError::NotFound("routes not found".into()))
        );
    }
}
