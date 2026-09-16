use crate::access_projection::edge_access;
use crate::modules::edge::application::{GetRoute, ListRoutes};
use crate::modules::edge::presentation::dto::RouteResponse;
use crate::modules::identity::presentation::{
    DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator,
    with_deferred_resource_scope,
};
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_id;

pub fn route_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List uses Nest macros; org-scoped get keeps deferred project admission.
    let mut controller = Arc::new(RouteQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/routes/{route_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(GetRoute {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: RouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                            access,
                        })
                        .await?
                    {
                        Ok(route) => BootResponse::json(&RouteResponse::from(route)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?,
        DeferredResourceScope::Project,
    )?)?;
    Ok(controller)
}

#[derive(Debug, Clone)]
struct RouteQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl RouteQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/routes",
        raw
    )]
    async fn list(&self, request: BootRequest) -> Result<BootResponse> {
        let request_id = request_id(&request)?;
        let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
        let environment_id =
            EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(ListRoutes {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(routes) => BootResponse::json(
                &routes
                    .into_iter()
                    .map(RouteResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_route_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn route_queries_controller_registers_list_via_nest_macros_and_deferred_get() {
        let controller = route_queries_controller(Arc::new(QueryBus::new()))
            .expect("route nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/routes"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path() == "/organizations/{organization_id}/routes/{route_id}"
        }));
    }
}
