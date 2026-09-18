use crate::access_projection::edge_access;
use crate::modules::edge::application::ListGatewayScopes;
use crate::modules::edge::presentation::dto::GatewayScopeResponse;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
};
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_id;

pub fn gateway_scope_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    Arc::new(GatewayScopeQueriesController { bus }).controller()
}

#[derive(Debug, Clone)]
struct GatewayScopeQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl GatewayScopeQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes",
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
            .execute(ListGatewayScopes {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(scopes) => BootResponse::json(
                &scopes
                    .into_iter()
                    .map(GatewayScopeResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_gateway_scope_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn gateway_scope_queries_controller_registers_guarded_get_via_nest_macros() {
        let controller = gateway_scope_queries_controller(Arc::new(QueryBus::new()))
            .expect("gateway scope nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method(), HttpMethod::Get);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/gateway-scopes"
        );
    }
}
