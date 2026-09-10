use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::{resource_access_evaluator, OrganizationTenantGuard};
use crate::modules::inference::application::{
    GetInferenceRoute, ListInferenceRoutes, DEFAULT_INFERENCE_ROUTE_LIST_LIMIT,
    MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT,
};
use crate::modules::inference::presentation::dto::{
    InferenceRoutePageResponse, InferenceRouteResponse,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceRouteId, OrganizationId, ProjectId,
};
use crate::presentation::{application_error_response, request_id};
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, QueryBus, Result, AUTH_SCOPES_METADATA,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ListInferenceRoutesQuery {
    cursor: Option<String>,
    limit: Option<usize>,
}

pub fn inference_route_queries_controller(bus: Arc<QueryBus>) -> Result<ControllerDefinition> {
    let list_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::INFERENCE_READ])?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes",
            move |request: BootRequest| {
                let bus = Arc::clone(&list_bus);
                async move {
                    let request_id = request_id(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    let parameters: ListInferenceRoutesQuery = request.query()?;
                    let limit = parameters
                        .limit
                        .unwrap_or(DEFAULT_INFERENCE_ROUTE_LIST_LIMIT)
                        .min(MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT);
                    let limit = if limit == 0 {
                        DEFAULT_INFERENCE_ROUTE_LIST_LIMIT
                    } else {
                        limit
                    };
                    match bus
                        .execute(ListInferenceRoutes {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            cursor: parameters.cursor,
                            limit,
                            resource_access,
                        })
                        .await?
                    {
                        Ok(page) => BootResponse::json(&InferenceRoutePageResponse::from(page)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .get(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes/{route_id}",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let request_id = request_id(&request)?;
                    let resource_access =
                        resource_access_evaluator(&request.require_auth_principal()?)?;
                    match bus
                        .execute(GetInferenceRoute {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            route_id: InferenceRouteId::from_uuid(
                                request.param_as::<Uuid>("route_id")?,
                            ),
                            resource_access,
                        })
                        .await?
                    {
                        Ok(route) => BootResponse::json(&InferenceRouteResponse::from(route)),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
