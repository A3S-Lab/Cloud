use crate::access_projection::inference_access;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{OrganizationTenantGuard, resource_access_evaluator};
use crate::modules::inference::application::{
    DEFAULT_INFERENCE_ROUTE_LIST_LIMIT, GetInferenceRoute, ListInferenceRoutes,
    MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT,
};
use crate::modules::inference::presentation::dto::{
    InferenceRoutePageResponse, InferenceRouteResponse,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceRouteId, OrganizationId, ProjectId,
};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    controller, get, metadata, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ListInferenceRoutesQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub fn inference_route_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(InferenceRouteQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct InferenceRouteQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::INFERENCE_READ])]
impl InferenceRouteQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = inference_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        let parameters: ListInferenceRoutesQuery = request.query()?;
        let limit = parameters
            .limit
            .unwrap_or(DEFAULT_INFERENCE_ROUTE_LIST_LIMIT)
            .min(MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT);
        let limit = if limit == 0 {
            DEFAULT_INFERENCE_ROUTE_LIST_LIMIT
        } else {
            limit
        };
        match self
            .bus
            .execute(ListInferenceRoutes {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                cursor: parameters.cursor,
                limit,
                access,
            })
            .await?
        {
            Ok(page) => BootResponse::json(&InferenceRoutePageResponse::from(page)),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes/{route_id}",
        raw
    )]
    async fn get_one(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let access = inference_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(GetInferenceRoute {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                route_id: InferenceRouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                access,
            })
            .await?
        {
            Ok(route) => BootResponse::json(&InferenceRouteResponse::from(route)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_inference_route_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;
    use std::collections::HashSet;

    #[test]
    fn inference_route_queries_register_scoped_guarded_gets_via_nest_macros() {
        let controller = inference_route_queries_controller(Arc::new(QueryBus::new()))
            .expect("inference route nest queries");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().all(|route| route.method() == HttpMethod::Get));
        let paths: HashSet<_> = routes.iter().map(|route| route.path().to_string()).collect();
        assert_eq!(
            paths,
            HashSet::from([
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes".to_string(),
                "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes/{route_id}".to_string(),
            ])
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::INFERENCE_READ])
        );
    }
}
