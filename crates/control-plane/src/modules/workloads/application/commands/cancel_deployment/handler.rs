use super::{CancelDeployment, CancelDeploymentResult};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, RepositoryError};
use crate::modules::workloads::domain::events::DeploymentCancellationRequested;
use crate::modules::workloads::domain::repositories::{
    IWorkloadRepository, RequestDeploymentCancellationBundle,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CancelDeploymentHandler {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl CancelDeploymentHandler {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }
}

impl CommandHandler<CancelDeployment> for CancelDeploymentHandler {
    fn execute(
        &self,
        command: CancelDeployment,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<CancelDeploymentResult>>>
    {
        let workloads = Arc::clone(&self.workloads);
        Box::pin(async move {
            let mut deployment = match workloads
                .find_deployment(command.organization_id, command.deployment_id)
                .await
            {
                Ok(deployment) => deployment,
                Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound(
                        "deployment not found".into(),
                    )))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "deploymentId": command.deployment_id,
                "organizationId": command.organization_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/deployments/{}/cancellation",
                    command.organization_id, command.deployment_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let expected_version = deployment.aggregate_version;
            if let Err(error) = deployment.request_cancellation(command.requested_at) {
                match workloads.replay_deployment_cancellation(&idempotency).await {
                    Ok(Some(replay))
                        if replay.organization_id == command.organization_id
                            && replay.id == command.deployment_id =>
                    {
                        return Ok(Ok(CancelDeploymentResult {
                            deployment: replay,
                            replayed: true,
                        }));
                    }
                    Ok(Some(_)) => {
                        return Err(BootError::Internal(
                            "deployment cancellation replay changed its identity".into(),
                        ));
                    }
                    Ok(None) => {}
                    Err(repository_error) => return Ok(Err(repository_error.into())),
                }
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = DeploymentCancellationRequested::envelope(&deployment, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match workloads
                .request_deployment_cancellation(RequestDeploymentCancellationBundle {
                    deployment,
                    expected_version,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(result) => Ok(Ok(CancelDeploymentResult {
                    deployment: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
