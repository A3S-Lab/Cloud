use super::RetireInferenceRoute;
use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::{
    IInferenceRouteRepository, RetireInferenceRouteWrite,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct RetireInferenceRouteHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    routes: Arc<dyn IInferenceRouteRepository>,
}

impl RetireInferenceRouteHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        routes: Arc<dyn IInferenceRouteRepository>,
    ) -> Self {
        Self {
            environments,
            routes,
        }
    }
}

impl CommandHandler<RetireInferenceRoute> for RetireInferenceRouteHandler {
    fn execute(
        &self,
        command: RetireInferenceRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceRoute>>> {
        let environments = Arc::clone(&self.environments);
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            if command.expected_aggregate_version == 0 {
                return Ok(Err(ApplicationError::Invalid(
                    "inference route expected_aggregate_version must be greater than 0".into(),
                )));
            }

            match environments
                .find(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "environment not found in organization and project".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            }

            let canonical = serde_json::to_vec(&CanonicalRetireInferenceRoute {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                route_id: command.route_id,
                expected_aggregate_version: command.expected_aggregate_version,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/inference/routes/{}/retire",
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.route_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };

            match routes
                .replay_inference_route_write(command.organization_id, &idempotency)
                .await
            {
                Ok(Some(write)) => return Ok(Ok(write.value)),
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }

            let mut route = match routes
                .find_inference_route(command.organization_id, command.route_id)
                .await
            {
                Ok(Some(route))
                    if route.project_id == command.project_id
                        && route.environment_id == command.environment_id =>
                {
                    route
                }
                Ok(_) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "inference route not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };

            if route.aggregate_version() != command.expected_aggregate_version {
                return Ok(Err(ApplicationError::Conflict(
                    "inference route changed before retirement".into(),
                )));
            }

            if let Err(error) = route.retire(command.requested_at) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }

            match routes
                .retire_inference_route(RetireInferenceRouteWrite {
                    route,
                    expected_aggregate_version: command.expected_aggregate_version,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(write.value)),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Serialize)]
struct CanonicalRetireInferenceRoute {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    route_id: crate::modules::shared_kernel::domain::InferenceRouteId,
    expected_aggregate_version: u64,
}
