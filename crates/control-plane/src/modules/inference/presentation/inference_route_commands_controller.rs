use crate::modules::identity::domain::value_objects::ApiTokenScope;
use crate::modules::identity::presentation::OrganizationTenantGuard;
use crate::modules::inference::application::{PublishInferenceRoute, RetireInferenceRoute};
use crate::modules::inference::presentation::dto::{
    InferenceRouteResponse, PublishInferenceRouteRequest,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceRouteId, OrganizationId, ProjectId,
};
use crate::presentation::{application_error_response, request_identity};
use a3s_boot::{
    BootError, BootRequest, BootResponse, CommandBus, ControllerDefinition, Result,
    AUTH_SCOPES_METADATA,
};
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

pub fn inference_route_commands_controller(bus: Arc<CommandBus>) -> Result<ControllerDefinition> {
    let publish_bus = Arc::clone(&bus);
    ControllerDefinition::new("/organizations")?
        .with_guard(OrganizationTenantGuard)
        .with_metadata(AUTH_SCOPES_METADATA, vec![ApiTokenScope::INFERENCE_WRITE])?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes",
            move |request: BootRequest| {
                let bus = Arc::clone(&publish_bus);
                async move {
                    let body: PublishInferenceRouteRequest = request.json_with_content_type()?;
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    let (router, models, grants, binding) = body
                        .into_parts()
                        .map_err(BootError::BadRequest)?;
                    match bus
                        .execute(PublishInferenceRoute {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            project_id: ProjectId::from_uuid(
                                request.param_as::<Uuid>("project_id")?,
                            ),
                            environment_id: EnvironmentId::from_uuid(
                                request.param_as::<Uuid>("environment_id")?,
                            ),
                            router,
                            models,
                            grants,
                            binding,
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(route) => Ok(BootResponse::json_with_status(
                            202,
                            &InferenceRouteResponse::from(route),
                        )?
                        .with_header("cache-control", "no-store")
                        .with_header("pragma", "no-cache")
                        .with_header("referrer-policy", "no-referrer")),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )?
        .post(
            "/{organization_id}/projects/{project_id}/environments/{environment_id}/inference/routes/{route_id}/retire",
            move |request: BootRequest| {
                let bus = Arc::clone(&bus);
                async move {
                    let (idempotency_key, request_id) = request_identity(&request)?;
                    match bus
                        .execute(RetireInferenceRoute {
                            organization_id: OrganizationId::from_uuid(
                                request.param_as::<Uuid>("organization_id")?,
                            ),
                            route_id: InferenceRouteId::from_uuid(
                                request.param_as::<Uuid>("route_id")?,
                            ),
                            idempotency_key,
                            request_id,
                            requested_at: Utc::now(),
                        })
                        .await?
                    {
                        Ok(route) => Ok(BootResponse::json_with_status(
                            202,
                            &InferenceRouteResponse::from(route),
                        )?
                        .with_header("cache-control", "no-store")
                        .with_header("pragma", "no-cache")
                        .with_header("referrer-policy", "no-referrer")),
                        Err(error) => application_error_response(error, request_id),
                    }
                }
            },
        )
}
