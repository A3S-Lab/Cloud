use super::{InferenceRoutePage, ListInferenceRoutes, MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT};
use crate::modules::inference::application::{
    IInferenceEnvironmentAccess, InferenceEnvironmentScope,
};
use crate::modules::inference::domain::repositories::IInferenceRouteRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::InferenceRouteId;
use a3s_boot::{CqrsContext, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

pub struct ListInferenceRoutesHandler {
    environments: Arc<dyn IInferenceEnvironmentAccess>,
    routes: Arc<dyn IInferenceRouteRepository>,
}

impl ListInferenceRoutesHandler {
    pub fn new(
        environments: Arc<dyn IInferenceEnvironmentAccess>,
        routes: Arc<dyn IInferenceRouteRepository>,
    ) -> Self {
        Self {
            environments,
            routes,
        }
    }
}

impl QueryHandler<ListInferenceRoutes> for ListInferenceRoutesHandler {
    fn execute(
        &self,
        query: ListInferenceRoutes,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceRoutePage>>> {
        let environments = Arc::clone(&self.environments);
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            if !query
                .access
                .environment_is_visible(query.project_id, query.environment_id)
            {
                return Ok(Err(ApplicationError::NotFound(
                    "environment not found in organization".into(),
                )));
            }
            let scope = match InferenceEnvironmentScope::new(
                query.organization_id,
                query.project_id,
                query.environment_id,
            ) {
                Ok(scope) => scope,
                Err(error) => return Ok(Err(ApplicationError::Forbidden(error))),
            };
            match environments.environment_exists(scope).await {
                Ok(true) => {}
                Ok(false) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )));
                }
                Err(error) => return Ok(Err(error.into())),
            }
            if query.limit == 0 || query.limit > MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "inference route list limit must be between 1 and {MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT}"
                ))));
            }
            let after = match query.cursor.as_deref().map(parse_route_cursor) {
                Some(Ok(route_id)) => Some(route_id),
                Some(Err(error)) => return Ok(Err(ApplicationError::Invalid(error))),
                None => None,
            };
            let mut listed = match routes
                .list_inference_routes_by_environment(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                )
                .await
            {
                Ok(routes) => routes,
                Err(error) => return Ok(Err(error.into())),
            };
            if let Some(after) = after {
                listed.retain(|route| route.id > after);
            }
            let next_cursor = (listed.len() > query.limit)
                .then(|| encode_route_cursor(listed[query.limit - 1].id));
            listed.truncate(query.limit);
            Ok(Ok(InferenceRoutePage {
                routes: listed,
                next_cursor,
            }))
        })
    }
}

fn encode_route_cursor(route_id: InferenceRouteId) -> String {
    route_id.as_uuid().to_string()
}

fn parse_route_cursor(raw: &str) -> Result<InferenceRouteId, String> {
    let uuid = Uuid::parse_str(raw).map_err(|_| "inference route cursor is invalid".to_owned())?;
    if uuid.is_nil() {
        return Err("inference route cursor is invalid".into());
    }
    Ok(InferenceRouteId::from_uuid(uuid))
}
