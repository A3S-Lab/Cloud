use super::request::{acl_document, request_identity};
use crate::modules::edge::application::{CreateMcpRoutePolicy, ReviseMcpRoutePolicy};
use crate::modules::edge::domain::MCP_ROUTE_POLICY_MAX_ACL_BYTES;
use crate::modules::edge::presentation::dto::McpRoutePolicyMutationResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, CommandBus, ControllerDefinition, Result, AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_route_policy_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let create_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::MCP_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies",
            move |request: BootRequest| {
                let bus = Arc::clone(&create_bus);
                async move {
                    let acl = acl_document(
                        &request,
                        MCP_ROUTE_POLICY_MAX_ACL_BYTES,
                        "MCP route policy",
                    )?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(CreateMcpRoutePolicy {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
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
            },
        )?
        .post(
            "/{organization_id}/mcp-route-policies/{route_id}/revisions",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let acl = acl_document(
                        &request,
                        MCP_ROUTE_POLICY_MAX_ACL_BYTES,
                        "MCP route policy",
                    )?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(ReviseMcpRoutePolicy {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: RouteId::from_uuid(
                                request.param_as::<Uuid>("route_id")?,
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
            },
        )
}
