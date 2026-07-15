use crate::modules::edge::application::{GetRoute, ListRoutes};
use crate::modules::edge::presentation::dto::RouteResponse;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId, RouteId};
use crate::presentation::application_error_response;
use a3s_boot::{BootError, BootRequest, BootResponse, ControllerDefinition, QueryBus, Result};
use std::sync::Arc;
use uuid::Uuid;

pub fn route_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let get_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/routes",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(ListRoutes {
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
                        Ok(routes) => BootResponse::json(
                            &routes
                                .into_iter()
                                .map(RouteResponse::from)
                                .collect::<Vec<_>>(),
                        ),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/routes/{route_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&get_bus);
                async move {
                    let request_id = request_id(&request)?;
                    match bus
                        .execute(GetRoute {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: RouteId::from_uuid(request.param_as::<Uuid>("route_id")?),
                        })
                        .await?
                    {
                        Ok(route) => BootResponse::json(&RouteResponse::from(route)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
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
