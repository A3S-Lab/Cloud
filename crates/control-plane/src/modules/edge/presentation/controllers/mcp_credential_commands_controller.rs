use crate::access_projection::edge_access;
use crate::modules::edge::application::{
    CreateMcpCredential, RevokeMcpCredential, RotateMcpCredential,
};
use crate::modules::edge::presentation::dto::{
    CreateMcpCredentialRequest, McpCredentialDeliveryResponse, McpCredentialMutationResponse,
    RevokeMcpCredentialRequest, RotateMcpCredentialRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::{OrganizationTenantGuard, resource_access_evaluator, application_error_response};
use a3s_boot::{
    controller, metadata, post, use_guard, AUTH_SCOPES_METADATA, BootRequest, BootResponse,
    CommandBus, ControllerDefinition, Result,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use super::request::request_identity;

pub fn mcp_credential_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    Arc::new(McpCredentialCommandsController { bus }).controller()
}

#[derive(Debug, Clone)]
struct McpCredentialCommandsController {
    bus: Arc<CommandBus>,
}

#[controller("/organizations")]
#[use_guard(OrganizationTenantGuard)]
#[metadata("auth.scopes", vec![ApiTokenScope::MCP_WRITE])]
impl McpCredentialCommandsController {
    #[post(
        "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
        raw
    )]
    async fn create(&self, request: BootRequest) -> Result<BootResponse> {
        let body: CreateMcpCredentialRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let requested_at = Utc::now();
        let access = edge_access(&resource_access_evaluator(
            &request.require_auth_principal()?,
        )?);
        match self
            .bus
            .execute(CreateMcpCredential {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                project_id: ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
                environment_id: EnvironmentId::from_uuid(
                    request.param_as::<Uuid>("environment_id")?,
                ),
                access,
                expires_at: body.expires_at,
                idempotency_key,
                request_id,
                requested_at,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                delivery_response(status, McpCredentialDeliveryResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/mcp-credentials/{credential_id}/rotate", raw)]
    async fn rotate(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RotateMcpCredentialRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        let requested_at = Utc::now();
        match self
            .bus
            .execute(RotateMcpCredential {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                credential_id: McpCredentialId::from_uuid(
                    request.param_as::<Uuid>("credential_id")?,
                ),
                expires_at: body.expires_at,
                expected_aggregate_version: body.expected_aggregate_version,
                idempotency_key,
                request_id,
                requested_at,
            })
            .await?
        {
            Ok(result) => {
                let status = if result.replayed { 200 } else { 201 };
                delivery_response(status, McpCredentialDeliveryResponse::from(result))
            }
            Err(error) => application_error_response(error, request_id),
        }
    }

    #[post("/{organization_id}/mcp-credentials/{credential_id}/revoke", raw)]
    async fn revoke(&self, request: BootRequest) -> Result<BootResponse> {
        let body: RevokeMcpCredentialRequest = request.json_with_content_type()?;
        let (idempotency_key, request_id) = request_identity(&request)?;
        match self
            .bus
            .execute(RevokeMcpCredential {
                organization_id: OrganizationId::from_uuid(
                    request.param_as::<Uuid>("organization_id")?,
                ),
                credential_id: McpCredentialId::from_uuid(
                    request.param_as::<Uuid>("credential_id")?,
                ),
                expected_aggregate_version: body.expected_aggregate_version,
                idempotency_key,
                request_id,
                requested_at: Utc::now(),
            })
            .await?
        {
            Ok(result) => BootResponse::json(&McpCredentialMutationResponse::from(result)),
            Err(error) => application_error_response(error, request_id),
        }
    }
}

fn delivery_response(status: u16, response: McpCredentialDeliveryResponse) -> Result<BootResponse> {
    Ok(BootResponse::json_with_status(status, &response)?
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer"))
}

#[cfg(test)]
mod nest_macro_mcp_credential_commands_controller_tests {
    use super::*;
    use a3s_boot::HttpMethod;

    #[test]
    fn mcp_credential_commands_controller_registers_scoped_posts_via_nest_macros() {
        let controller = mcp_credential_commands_controller(Arc::new(CommandBus::new()))
            .expect("mcp credential nest command controller");
        assert_eq!(controller.prefix(), "/organizations");
        let routes = controller.routes();
        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].method(), HttpMethod::Post);
        assert_eq!(
            routes[0].path(),
            "/organizations/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials"
        );
        assert_eq!(
            routes[1].path(),
            "/organizations/{organization_id}/mcp-credentials/{credential_id}/rotate"
        );
        assert_eq!(
            routes[2].path(),
            "/organizations/{organization_id}/mcp-credentials/{credential_id}/revoke"
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
