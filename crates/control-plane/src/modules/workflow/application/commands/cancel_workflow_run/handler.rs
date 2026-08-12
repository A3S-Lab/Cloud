use super::CancelWorkflowRun;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use crate::modules::workflow::application::resource_access;
use crate::modules::workflow::application::WorkflowRunMutationResult;
use crate::modules::workflow::domain::{
    CancelWorkflowRunWrite, IWorkflowRunRepository, WorkflowRunCancellationRequested,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use std::sync::Arc;

pub struct CancelWorkflowRunHandler {
    runs: Arc<dyn IWorkflowRunRepository>,
}

impl CancelWorkflowRunHandler {
    pub fn new(runs: Arc<dyn IWorkflowRunRepository>) -> Self {
        Self { runs }
    }
}

impl CommandHandler<CancelWorkflowRun> for CancelWorkflowRunHandler {
    fn execute(
        &self,
        command: CancelWorkflowRun,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<WorkflowRunMutationResult>>>
    {
        let runs = Arc::clone(&self.runs);
        Box::pin(async move {
            let mut record = match resource_access::workflow_run(
                runs.as_ref(),
                command.organization_id,
                command.workflow_run_id,
                &command.resource_access,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let canonical = serde_json::to_vec(&serde_json::json!({
                "organizationId": command.organization_id,
                "workflowRunId": command.workflow_run_id,
                "reason": command.reason,
            }))
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                format!(
                    "organizations/{}/workflow-runs/{}/cancellation",
                    command.organization_id, command.workflow_run_id
                ),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            if let Some(record) = match runs.replay(&idempotency).await {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            } {
                return Ok(Ok(WorkflowRunMutationResult {
                    record,
                    replayed: true,
                }));
            }
            let expected_version = record.run.aggregate_version;
            if let Err(error) = record.run.request_cancellation(
                command.reason,
                command.actor_principal_id,
                command.requested_at,
            ) {
                return Ok(Err(ApplicationError::Conflict(error)));
            }
            let event = WorkflowRunCancellationRequested::envelope(&record.run, command.request_id)
                .map_err(BootError::Internal)?;
            match runs
                .request_cancellation(CancelWorkflowRunWrite {
                    record,
                    expected_version,
                    event,
                    actor_principal_id: command.actor_principal_id,
                    request_id: command.request_id,
                    idempotency,
                })
                .await
            {
                Ok(write) => Ok(Ok(WorkflowRunMutationResult {
                    record: write.value,
                    replayed: write.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}
