use super::request::{acl_document, request_identity};
use crate::modules::edge::application::{CreateMcpRoutePolicy, ReviseMcpRoutePolicy};
use crate::modules::edge::domain::MCP_ROUTE_POLICY_MAX_ACL_BYTES;
use crate::modules::edge::presentation::dto::McpRoutePolicyMutationResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::application_error_response;
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_route_policy_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(McpRoutePolicyCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct McpRoutePolicyCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::MCP_WRITE])]
impl McpRoutePolicyCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let acl = acl_document(&request, MCP_ROUTE_POLICY_MAX_ACL_BYTES, "MCP route policy")?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(CreateMcpRoutePolicy {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                acl,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(write) => BootResponse::json_with_status(
                if write.replayed { 200 } else { 201 },
                &McpRoutePolicyMutationResponse::from(write),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/mcp-route-policies/{route_id}/revisions", raw)]
    async fn revise(&self, request: BootRequest) -> Result<BootResponse> {
        let acl = acl_document(&request, MCP_ROUTE_POLICY_MAX_ACL_BYTES, "MCP route policy")?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(ReviseMcpRoutePolicy {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                route_id: RouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                acl,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(write) => BootResponse::json_with_status(
                if write.replayed { 200 } else { 201 },
                &McpRoutePolicyMutationResponse::from(write),
            ),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

#[cfg(test)]
mod nest_macro_mcp_route_policy_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn mcp_route_policy_commands_controller_registers_scoped_posts_via_nest_macros() {
        let controller = mcp_route_policy_commands_controller(Arc::new(CommandBus::new()))
            .expect("mcp route policy nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/mcp-route-policies/{route_id}/revisions"
        );
        assert_eq!(
            routes[0]
                .metadata()
                .get(AUTH_SCOPES_METADATA)
                .cloned()
                .expect("auth.scopes"),
            serde_json::json!([ApiTokenScope::MCP_WRITE])
        );
    }
}
