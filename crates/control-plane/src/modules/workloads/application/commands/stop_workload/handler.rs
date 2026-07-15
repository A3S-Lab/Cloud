use super::{StopWorkload, StopWorkloadResult};
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{IdempotencyRequest, OperationId, RepositoryError};
use crate::modules::workloads::domain::events::WorkloadStopRequested;
use crate::modules::workloads::domain::repositories::{
    IWorkloadRepository, RequestWorkloadStopBundle,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct StopWorkloadHandler {
    workloads: Arc<dyn IWorkloadRepository>,
}

impl StopWorkloadHandler {
    pub fn new(workloads: Arc<dyn IWorkloadRepository>) -> Self {
        Self { workloads }
    }
}

impl CommandHandler<StopWorkload> for StopWorkloadHandler {
    fn execute(
        &self,
        command: StopWorkload,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<StopWorkloadResult>>> {
        let workloads = Arc::clone(&self.workloads);
        Box::pin(async move {
            let mut workload = match workloads
                .find_workload(command.organization_id, command.workload_id)
                .await
            {
                Ok(workload) => workload,
                Err(RepositoryError::NotFound) => {
                    return Ok(Err(ApplicationError::NotFound("workload not found".into())))
                }
                Err(error) => return Ok(Err(error.into())),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workloadId": command.workload_id,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workloads/{}/stop",
                    command.organization_id, command.workload_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(idempotency) => idempotency,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let expected_version = workload.aggregate_version;
            if let Err(error) = workload.request_stop(command.requested_at) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let operation_id = OperationId::new();
            let operation = OperationRequest::new(
                operation_id,
                workload.organization_id,
                OperationSubject::new("workload", workload.id.as_uuid())
                    .map_err(BootError::Internal)?,
                WorkflowIdentity::new("cloud.workload.stop", "1").map_err(BootError::Internal)?,
                serde_json::json!({
                    "operationId": operation_id,
                    "organizationId": workload.organization_id,
                    "requestedAt": command.requested_at,
                    "workloadId": workload.id,
                }),
                command.requested_at,
            );
            let event = WorkloadStopRequested::envelope(&workload, &operation, command.request_id)
                .map_err(|error| BootError::Internal(error.to_string()))?;
            match workloads
                .request_workload_stop(RequestWorkloadStopBundle {
                    workload,
                    expected_version,
                    operation,
                    idempotency,
                    event,
                })
                .await
            {
                Ok(bundle) => Ok(Ok(StopWorkloadResult { bundle })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
