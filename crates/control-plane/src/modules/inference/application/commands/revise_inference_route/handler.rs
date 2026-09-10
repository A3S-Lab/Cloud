use super::ReviseInferenceRoute;
use crate::modules::inference::application::{
    InferenceEdgeRouteBindingAdmissionRequest, InferenceGrantCredentialAdmissionRequest,
    IInferenceEdgeRouteBindingAdmissionPort, IInferenceGrantCredentialAdmissionPort,
};
use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::{
    IInferenceRouteRepository, ReviseInferenceRouteWrite,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct ReviseInferenceRouteHandler {
    environments: Arc<dyn IEnvironmentRepository>,
    routes: Arc<dyn IInferenceRouteRepository>,
    edge_route_bindings: Arc<dyn IInferenceEdgeRouteBindingAdmissionPort>,
    grant_credentials: Arc<dyn IInferenceGrantCredentialAdmissionPort>,
}

impl ReviseInferenceRouteHandler {
    pub fn new(
        environments: Arc<dyn IEnvironmentRepository>,
        routes: Arc<dyn IInferenceRouteRepository>,
        edge_route_bindings: Arc<dyn IInferenceEdgeRouteBindingAdmissionPort>,
        grant_credentials: Arc<dyn IInferenceGrantCredentialAdmissionPort>,
    ) -> Self {
        Self {
            environments,
            routes,
            edge_route_bindings,
            grant_credentials,
        }
    }
}

impl CommandHandler<ReviseInferenceRoute> for ReviseInferenceRouteHandler {
    fn execute(
        &self,
        command: ReviseInferenceRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceRoute>>> {
        let environments = Arc::clone(&self.environments);
        let routes = Arc::clone(&self.routes);
        let edge_route_bindings = Arc::clone(&self.edge_route_bindings);
        let grant_credentials = Arc::clone(&self.grant_credentials);
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

            match edge_route_bindings
                .admit(InferenceEdgeRouteBindingAdmissionRequest::new(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.binding.clone(),
                ))
                .await
            {
                Ok(()) => {}
                Err(error) => return Ok(Err(error)),
            }

            match grant_credentials
                .admit(InferenceGrantCredentialAdmissionRequest::new(
                    command.organization_id,
                    command.project_id,
                    command.environment_id,
                    command.grants.clone(),
                    command.requested_at,
                ))
                .await
            {
                Ok(()) => {}
                Err(error) => return Ok(Err(error)),
            }

            let canonical = serde_json::to_vec(&CanonicalReviseInferenceRoute {
                organization_id: command.organization_id,
                project_id: command.project_id,
                environment_id: command.environment_id,
                route_id: command.route_id,
                expected_aggregate_version: command.expected_aggregate_version,
                router: &command.router,
                models: &command.models,
                grants: &command.grants,
                binding: &command.binding,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/projects/{}/environments/{}/inference/routes/{}/revisions",
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
                    "inference route changed before revision".into(),
                )));
            }

            if let Err(error) = route.revise(
                command.router,
                command.models,
                command.grants,
                command.binding,
                command.requested_at,
            ) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }

            match routes
                .revise_inference_route(ReviseInferenceRouteWrite {
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
struct CanonicalReviseInferenceRoute<'a> {
    organization_id: crate::modules::shared_kernel::domain::OrganizationId,
    project_id: crate::modules::shared_kernel::domain::ProjectId,
    environment_id: crate::modules::shared_kernel::domain::EnvironmentId,
    route_id: crate::modules::shared_kernel::domain::InferenceRouteId,
    expected_aggregate_version: u64,
    router: &'a str,
    models: &'a [a3s_cloud_contracts::InferenceModelAclProjection],
    grants: &'a [a3s_cloud_contracts::InferenceGrantAclProjection],
    binding: &'a crate::modules::inference::domain::value_objects::EdgeRouteBindingRef,
}
