use super::GetInferenceRoute;
use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::IInferenceRouteRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;

pub struct GetInferenceRouteHandler {
    routes: Arc<dyn IInferenceRouteRepository>,
}

impl GetInferenceRouteHandler {
    pub fn new(routes: Arc<dyn IInferenceRouteRepository>) -> Self {
        Self { routes }
    }
}

impl QueryHandler<GetInferenceRoute> for GetInferenceRouteHandler {
    fn execute(
        &self,
        query: GetInferenceRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceRoute>>> {
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            if !query
                .resource_access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            match routes
                .find_inference_route(query.organization_id, query.route_id)
                .await
            {
                Ok(Some(route))
                    if route.project_id == query.project_id
                        && route.environment_id == query.environment_id =>
                {
                    Ok(Ok(route))
                }
                Ok(_) => Ok(Err(ApplicationError::NotFound(
                    "inference route not found".into(),
                ))),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
