use super::mcp_credential_response_security::{no_store, McpCredentialNoStoreErrorFilter};
use crate::modules::edge::application::{GetMcpCredential, ListMcpCredentials};
use crate::modules::edge::presentation::dto::McpCredentialResponse;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, McpCredentialId, OrganizationId, ProjectId,
};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_credential_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_filter(McpCredentialNoStoreErrorFilter)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (organization_id, project_id, environment_id) =
                        credential_scope(&request)?;
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListMcpCredentials {
                            organization_id,
                            project_id,
                            environment_id,
                        })
                        .await?
                    {
                        Ok(credentials) => Ok(no_store(BootResponse::json(
                            &credentials
                                .into_iter()
                                .map(McpCredentialResponse::from)
                                .collect::<Vec<_>>(),
                        )?)),
                        Err(error) => {
                            Ok(no_store(application_error_response(error, request_id)?))
                        }
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials/{credential_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let (organization_id, project_id, environment_id) =
                        credential_scope(&request)?;
                    let credential_id = McpCredentialId::from_uuid(
                        request.param_as::<Uuid>("credential_id")?,
                    );
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMcpCredential {
                            organization_id,
                            project_id,
                            environment_id,
                            credential_id,
                        })
                        .await?
                    {
                        Ok(credential) => Ok(no_store(BootResponse::json(
                            &McpCredentialResponse::from(credential),
                        )?)),
                        Err(error) => {
                            Ok(no_store(application_error_response(error, request_id)?))
                        }
                    }
                }
            },
        )
}

fn credential_scope(request: &BootRequest) -> Result<(OrganizationId, ProjectId, EnvironmentId)> {
    Ok((
        OrganizationId::from_uuid(request.param_as::<Uuid>("organization_id")?),
        ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?),
        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?),
    ))
}

fn request_id(request: &BootRequest) -> Result<Uuid> {
    request
        .header("x-request-id")
        .ok_or_else(|| BootError::Internal("request ID middleware did not run".into()))
        .and_then(|value| {
            Uuid::parse_str(value)
                .map_err(|error| BootError::Internal(format!("invalid request ID: {error}")))
        })
}
