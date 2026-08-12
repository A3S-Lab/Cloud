use super::CancelExecutionResult;
use crate::modules::executions::domain::events::ExecutionCancellationRequested;
use crate::modules::executions::domain::{Execution, IExecutionRepository, TransitionExecution};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::IdempotencyRequest;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct ExecutionCancellation {
    pub execution: Execution,
    pub idempotency_key: String,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

#[derive(Clone)]
pub(crate) struct ExecutionCancellationService {
    executions: Arc<dyn IExecutionRepository>,
}

impl ExecutionCancellationService {
    pub fn new(executions: Arc<dyn IExecutionRepository>) -> Self {
        Self { executions }
    }

    pub async fn cancel(
        &self,
        request: ExecutionCancellation,
    ) -> ApplicationResult<CancelExecutionResult> {
        let mut execution = request.execution;
        let canonical = serde_json::to_vec(&serde_json::json!({
            "organizationId": execution.organization_id,
            "executionId": execution.id,
        }))
        .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let idempotency = IdempotencyRequest::new(
            format!(
                "organizations/{}/executions/{}/cancellation",
                execution.organization_id, execution.id
            ),
            request.idempotency_key,
            &canonical,
        )
        .map_err(ApplicationError::Invalid)?;
        if let Some(replay) = self.executions.replay(&idempotency).await? {
            if replay.organization_id != execution.organization_id
                || replay.project_id != execution.project_id
                || replay.environment_id != execution.environment_id
                || replay.id != execution.id
                || replay.workflow != execution.workflow
            {
                return Err(ApplicationError::Internal(
                    "execution cancellation replay changed its immutable identity".into(),
                ));
            }
            return Ok(CancelExecutionResult {
                execution: replay,
                replayed: true,
            });
        }
        let expected_version = execution.aggregate_version;
        execution
            .request_cancellation(request.requested_at)
            .map_err(ApplicationError::Conflict)?;
        let event = ExecutionCancellationRequested::envelope(&execution, request.request_id)
            .map_err(|error| ApplicationError::Internal(error.to_string()))?;
        let write = self
            .executions
            .request_cancellation(TransitionExecution {
                execution,
                expected_version,
                idempotency,
                event,
            })
            .await?;
        Ok(CancelExecutionResult {
            execution: write.execution,
            replayed: write.replayed,
        })
    }
}
