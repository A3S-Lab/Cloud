use super::mcp_credential_response_security::{no_store, McpCredentialNoStoreErrorFilter};
use crate::modules::edge::application::{
    IssueMcpCredential, McpCredentialMutationResult, RevokeMcpCredential, RotateMcpCredential,
};
use crate::modules::edge::presentation::dto::{
    IssueMcpCredentialRequest, McpCredentialMutationResponse, RotateMcpCredentialRequest,
};
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::{api_success_envelope, application_error_response};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_credential_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let rotate_bus = Arc::clone(&bus);
    let revoke_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_filter(McpCredentialNoStoreErrorFilter)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::ROUTE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let body: IssueMcpCredentialRequest = request.json_with_content_type()?;
                    let (organization_id, project_id, environment_id) =
                        credential_scope(&request)?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(IssueMcpCredential {
                            organization_id,
                            project_id,
                            environment_id,
                            expires_at: body.expires_at,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => mutation_response(
                            if result.replayed { 200 } else { 201 },
                            result,
                            request_id,
                        ),
                        Err(error) => {
                            Ok(no_store(application_error_response(error, request_id)?))
                        }
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials/{credential_id}/rotate",
            move |request: BootRequest| {
                let bus = Arc::clone(&rotate_bus);
                async move {
                    let body: RotateMcpCredentialRequest = request.json_with_content_type()?;
                    let (organization_id, project_id, environment_id) =
                        credential_scope(&request)?;
                    let credential_id = McpCredentialId::from_uuid(
                        request.param_as::<Uuid>("credential_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RotateMcpCredential {
                            organization_id,
                            project_id,
                            environment_id,
                            credential_id,
                            expires_at: body.expires_at,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => mutation_response(200, result, request_id),
                        Err(error) => {
                            Ok(no_store(application_error_response(error, request_id)?))
                        }
                    }
                }
            },
        )?
        .delete(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials/{credential_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&revoke_bus);
                async move {
                    let (organization_id, project_id, environment_id) =
                        credential_scope(&request)?;
                    let credential_id = McpCredentialId::from_uuid(
                        request.param_as::<Uuid>("credential_id")?,
                    );
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RevokeMcpCredential {
                            organization_id,
                            project_id,
                            environment_id,
                            credential_id,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(result) => mutation_response(200, result, request_id),
                        Err(error) => {
                            Ok(no_store(application_error_response(error, request_id)?))
                        }
                    }
                }
            },
        )
}

fn mutation_response(
    status: u16,
    result: McpCredentialMutationResult,
    request_id: Uuid,
) -> Result<BootResponse> {
    let envelope = api_success_envelope(
        status,
        McpCredentialMutationResponse::from(result),
        request_id,
    );
    Ok(no_store(
        BootResponse::json_with_status(status, &envelope)?.with_header("x-a3s-api-envelope", "1"),
    ))
}

fn credential_scope(request: &BootRequest) -> Result<(OrganizationId, ProjectId, EnvironmentId)> {
    Ok((
        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
        ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?),
    ))
}

fn request_identity(request: &BootRequest) -> Result<(String, Uuid)> {
    let idempotency_key = request
        .header("idempotency-key")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| BootError::BadRequest("idempotency-key header is required".into()))?
        .to_owned();
    let request_id = request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })?;
    Ok((idempotency_key, request_id))
}
