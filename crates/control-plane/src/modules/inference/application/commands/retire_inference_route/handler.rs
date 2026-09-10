use super::RetireInferenceRoute;
use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::domain::repositories::{
    RetireInferenceRouteWrite, IInferenceRouteRepository,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use serde::Serialize;
use std::sync::Arc;

pub struct RetireInferenceRouteHandler {
    routes: Arc<dyn IInferenceRouteRepository>,
}

impl RetireInferenceRouteHandler {
    pub fn new(routes: Arc<dyn IInferenceRouteRepository>) -> Self {
        Self { routes }
    }
}

impl CommandHandler<RetireInferenceRoute> for RetireInferenceRouteHandler {
    fn execute(
        &self,
        command: RetireInferenceRoute,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<InferenceRoute>>> {
        let routes = Arc::clone(&self.routes);
        Box::pin(async move {
            let canonical = serde_json::to_vec(&CanonicalRetireInferenceRoute {
                organization_id: command.organization_id,
                route_id: command.route_id,
            })
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/inference/routes/{}/retire",
                    command.organization_id, command.route_id
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
                Ok(Some(route)) => route,
                Ok(None) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "inference route not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let expected_aggregate_version = route.aggregate_version();
            if let Err(error) = route.retire(command.requested_at) {
                return Ok(Err(ApplicationError::Invalid(error)));
            }

            match routes
                .retire_inference_route(RetireInferenceRouteWrite {
                    route,
                    expected_aggregate_version,
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
    route_id: crate::modules::shared_kernel::domain::InferenceRouteId,
}
