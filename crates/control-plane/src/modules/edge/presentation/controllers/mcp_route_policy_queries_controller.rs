use super::request::request_id;
use crate::modules::edge::application::{GetMcpRoutePolicy, ListMcpRoutePolicies};
use crate::modules::edge::presentation::dto::McpRoutePolicyResponse;
use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::application_error_response;
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use std::sync::Arc;
use uuid::Uuid;

pub fn mcp_route_policy_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::MCP_READ])?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/mcp-route-policies",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListMcpRoutePolicies {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
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
            },
        )?
        .get(
            "/{organization_id}/mcp-route-policies/{route_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetMcpRoutePolicy {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: RouteId::from_uuid(
                                request.param_as::<Uuid>("route_id")?,
                            ),
                        })
                        .await?
                    {
                        Ok(policy) => BootResponse::json(&McpRoutePolicyResponse::from(policy)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
