use crate::access_projection::edge_access;
use crate::modules::edge::application::{GetMcpCredential, ListMcpCredentials};
use crate::modules::edge::presentation::dto::McpCredentialResponse;
use crate::modules::identity::presentation::{
    DeferredResourceScope, OrganizationTenantGuard, resource_access_evaluator,
    with_deferred_resource_scope,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, get, use_guard, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result,
    RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_id;

pub fn mcp_credential_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    // List uses Nest macros; org-scoped get keeps deferred project admission.
    let mut controller = Arc::new(McpCredentialQueriesController {
        bus: Arc::clone(&bus),
    })
    .controller()?;
    controller = controller.route(with_deferred_resource_scope(
        RouteDefinition::get(
            "/{organization_id}/mcp-credentials/{credential_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
                        .execute(GetMcpCredential {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            credential_id: McpCredentialId::from_uuid(
                                request.param_as::<Uuid>("credential_id")?,
                            ),
                            access,
                        })
                        .await?
                    {
                        Ok(credential) => {
                            BootResponse::json(&McpCredentialResponse::from(credential))
                        }
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
struct McpCredentialQueriesController {
    bus: Arc<QueryBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
impl McpCredentialQueriesController {
    #[get(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
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
            .execute(ListMcpCredentials {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id,
                environment_id,
                access,
            })
            .await?
        {
            Ok(credentials) => BootResponse::json(
                &credentials
                    .into_iter()
                    .map(McpCredentialResponse::from)
                    .collect::<Vec<_>>(),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_mcp_credential_queries_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn mcp_credential_queries_controller_registers_list_via_nest_and_deferred_get() {
        let controller = mcp_credential_queries_controller(Arc::new(QueryBus::new()))
            .expect("mcp credential nest query controller");

        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials"
        }));
        assert!(routes.iter().any(|route| {
            route.method() == HttpMethod::Get
                && route.path()
                    == "/organizations/{organization_id}/mcp-credentials/{credential_id}"
        }));
    }
}
