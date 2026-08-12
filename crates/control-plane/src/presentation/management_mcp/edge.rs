use super::arguments::{EnvironmentScopeArguments, RouteArguments};
use super::tool_result;
use crate::modules::edge::presentation::RouteResponse;
use crate::modules::edge::{GetRoute, ListRoutes};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use a3s_boot::{QueryBus, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn list_routes(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: EnvironmentScopeArguments,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(ListRoutes {
            organization_id,
            project_id: ProjectId::from_uuid(arguments.project_id),
            environment_id: EnvironmentId::from_uuid(arguments.environment_id),
        })
        .await?
    {
        Ok(routes) => tool_result::success(
            200,
            routes
                .into_iter()
                .map(RouteResponse::from)
                .collect::<Vec<_>>(),
            request_id,
        ),
        Err(error) => tool_result::application_error(error, request_id),
    }
}

pub async fn get_route(
    bus: Arc<QueryBus>,
    organization_id: OrganizationId,
    arguments: RouteArguments,
    resource_access: ResourceAccessEvaluator,
    request_id: Uuid,
) -> Result<Value> {
    match bus
        .execute(GetRoute {
            organization_id,
            route_id: RouteId::from_uuid(arguments.route_id),
            resource_access,
        })
        .await?
    {
        Ok(route) => tool_result::success(200, RouteResponse::from(route), request_id),
        Err(error) => tool_result::application_error(error, request_id),
    }
}
