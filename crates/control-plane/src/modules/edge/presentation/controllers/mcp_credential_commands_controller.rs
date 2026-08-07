use crate::modules::edge::application::{
    CreateMcpCredential, RevokeMcpCredential, RotateMcpCredential,
};
use crate::modules::edge::presentation::dto::{
    CreateMcpCredentialRequest, McpCredentialDeliveryResponse, McpCredentialMutationResponse,
    RevokeMcpCredentialRequest, RotateMcpCredentialRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_credential_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    let rotate_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::MCP_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let body: CreateMcpCredentialRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let requested_at = Utc::now();
                    match bus
                        .execute(CreateMcpCredential {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
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
            },
        )?
        .post(
            "/{organization_id}/mcp-credentials/{credential_id}/rotate",
            move |request: BootRequest| {
                let bus = Arc::clone(&rotate_bus);
                async move {
                    let body: RotateMcpCredentialRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let requested_at = Utc::now();
                    match bus
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
            },
        )?
        .post(
            "/{organization_id}/mcp-credentials/{credential_id}/revoke",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: RevokeMcpCredentialRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
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
                        Ok(result) => {
                            BootResponse::json(&McpCredentialMutationResponse::from(result))
                        }
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}

fn delivery_response(status: u16, response: McpCredentialDeliveryResponse) -> Result<BootResponse> {
    Ok(BootResponse::json_with_status(status, &response)?
        .with_header("cache-control", "no-store")
        .with_header("pragma", "no-cache")
        .with_header("referrer-policy", "no-referrer"))
}

use super::request::request_identity;
