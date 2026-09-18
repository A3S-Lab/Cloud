use crate::access_projection::edge_access;
use crate::modules::edge::application::{GetMcpRoutePolicy, ListMcpRoutePolicies};
use crate::modules::edge::presentation::dto::McpRoutePolicyResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::{DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator, with_deferred_resource_scope, application_error_response};
use a3s_boot::{
    controller, get, metadata, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_id;

pub fn mcp_route_policy_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List uses Nest macros; org-scoped get keeps deferred project admission.
    let mut controller = Arc::new(McpRoutePolicyQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/mcp-route-policies/{route_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(GetMcpRoutePolicy {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: RouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                            access,
                        })
                        .await?
                    {
                        Ok(policy) => BootResponse::json(&McpRoutePolicyResponse::from(policy)),
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
struct McpRoutePolicyQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::CLOUD_READ])]
impl McpRoutePolicyQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies",
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
            .execute(ListMcpRoutePolicies {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(policies) => BootResponse::json(
                &policies
                    .into_iter()
                    .map(McpRoutePolicyResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_mcp_route_policy_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn mcp_route_policy_queries_controller_registers_list_via_nest_and_deferred_get() {
        let controller = mcp_route_policy_queries_controller(Arc::new(QueryBus::new()))
            .expect("mcp route policy nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/mcp-route-policies/{route_id}"
        }));
        let list = routes
            .iter()
            .find(|route| {
                route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies"
            })
            .expect("list route");
        assert_eq!(
            list.metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::CLOUD_READ])
        );
    }
}
