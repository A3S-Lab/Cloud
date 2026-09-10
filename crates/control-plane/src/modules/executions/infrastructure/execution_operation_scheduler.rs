use crate::modules::executions::application::{
    ExecutionOperationRequest, ExecutionOperationScheduleOutcome, ExecutionReconciler,
    IExecutionOperationScheduler, EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Sole anti-corruption adapter from Executions scheduling intent to Operations.
struct OperationsExecutionOperationScheduler {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsExecutionOperationScheduler {
    fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IExecutionOperationScheduler for OperationsExecutionOperationScheduler {
    async fn schedule(
        &self,
        request: ExecutionOperationRequest,
    ) -> Result<ExecutionOperationScheduleOutcome, RepositoryError> {
        let operation = OperationRequest::new(
            request.operation_id(),
            request.organization_id(),
            OperationSubject::new("execution", request.execution_id().as_uuid())
                .map_err(RepositoryError::Storage)?,
            WorkflowIdentity::new(EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION)
                .map_err(RepositoryError::Storage)?,
            json!({
                "organizationId": request.organization_id(),
                "executionId": request.execution_id(),
            }),
            request.requested_at(),
        );
        self.operations
            .enqueue(operation)
            .await
            .map(|write| ExecutionOperationScheduleOutcome::new(write.replayed))
    }
}

// Preserve the root composition API while keeping Operations types outside
// the Executions Application layer. New use cases inject IExecutionOperationScheduler
// directly; the composition root receives this Infrastructure convenience.
impl ExecutionReconciler {
    pub fn new(
        executions: Arc<dyn IExecutionRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self::from_operation_scheduler(
            executions,
            Arc::new(OperationsExecutionOperationScheduler::new(operations)),
        )
    }

    pub fn with_schedule(
        executions: Arc<dyn IExecutionRepository>,
        operations: Arc<dyn IOperationRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        Self::with_operation_scheduler_and_schedule(
            executions,
            Arc::new(OperationsExecutionOperationScheduler::new(operations)),
            interval,
            batch_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{ExecutionId, OperationId, OrganizationId};
    use chrono::Utc;

    #[tokio::test]
    async fn adapter_maps_one_execution_intent_and_preserves_operation_replay() {
        let operations = Arc::new(InMemoryOperationRepository::new());
        let scheduler = OperationsExecutionOperationScheduler::new(operations.clone());
        let execution_id = ExecutionId::new();
        let operation_id = OperationId::from_uuid(execution_id.as_uuid());
        let organization_id = OrganizationId::new();
        let request =
            ExecutionOperationRequest::new(operation_id, organization_id, execution_id, Utc::now());

        assert!(!scheduler
            .schedule(request)
            .await
            .expect("schedule")
            .replayed());
        assert!(scheduler
            .schedule(request)
            .await
            .expect("replay")
            .replayed());

        let operation = operations
            .find_request(operation_id)
            .await
            .expect("read operation")
            .expect("operation exists");
        assert_eq!(operation.organization_id, organization_id);
        assert_eq!(operation.subject.kind(), "execution");
        assert_eq!(operation.subject.id(), execution_id.as_uuid());
        assert_eq!(operation.workflow.name(), EXECUTION_WORKFLOW_NAME);
        assert_eq!(operation.workflow.version(), EXECUTION_WORKFLOW_VERSION);
        assert_eq!(
            operation.input["organizationId"],
            serde_json::json!(organization_id)
        );
        assert_eq!(
            operation.input["executionId"],
            serde_json::json!(execution_id)
        );
    }
}
