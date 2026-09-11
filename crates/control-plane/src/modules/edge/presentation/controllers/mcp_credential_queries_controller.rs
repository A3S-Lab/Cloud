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
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, RouteDefinition,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_credential_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-credentials",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let project_id = ProjectId::from_uuid(request.param_as::<Uuid>("project_id")?);
                    let environment_id =
                        EnvironmentId::from_uuid(request.param_as::<Uuid>("environment_id")?);
                    let access = edge_access(&resource_access_evaluator(
                        &request.require_auth_principal()?,
                    )?);
                    match bus
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
            },
        )?
        .route(with_deferred_resource_scope(
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
        )?)
}

use super::request::request_id;
