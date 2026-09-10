use crate::modules::agents::application::{
    AgentExecutionOperationRequest, AgentExecutionOperationScheduleOutcome, AgentExecutionReconciler,
    IAgentExecutionOperationScheduler, AGENT_EXECUTION_WORKFLOW_NAME,
    AGENT_EXECUTION_WORKFLOW_VERSION,
};
use crate::modules::agents::domain::IAgentRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Sole anti-corruption adapter from Agents scheduling intent to Operations.
struct OperationsAgentExecutionOperationScheduler {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsAgentExecutionOperationScheduler {
    fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IAgentExecutionOperationScheduler for OperationsAgentExecutionOperationScheduler {
    async fn schedule(
        &self,
        request: AgentExecutionOperationRequest,
    ) -> Result<AgentExecutionOperationScheduleOutcome, RepositoryError> {
        let operation = OperationRequest::new(
            request.operation_id(),
            request.organization_id(),
            OperationSubject::new("agent_execution", request.execution_id().as_uuid())
                .map_err(RepositoryError::Storage)?,
            WorkflowIdentity::new(AGENT_EXECUTION_WORKFLOW_NAME, AGENT_EXECUTION_WORKFLOW_VERSION)
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
            .map(|write| AgentExecutionOperationScheduleOutcome::new(write.replayed))
    }
}

// Preserve the root composition API while keeping Operations types outside
// the Agents Application layer. New use cases inject IAgentExecutionOperationScheduler
// directly; the composition root receives this Infrastructure convenience.
impl AgentExecutionReconciler {
    pub fn new(
        agents: Arc<dyn IAgentRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self::from_operation_scheduler(
            agents,
            Arc::new(OperationsAgentExecutionOperationScheduler::new(operations)),
        )
    }

    pub fn with_schedule(
        agents: Arc<dyn IAgentRepository>,
        operations: Arc<dyn IOperationRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        Self::with_operation_scheduler_and_schedule(
            agents,
            Arc::new(OperationsAgentExecutionOperationScheduler::new(operations)),
            interval,
            batch_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{AgentExecutionId, OperationId, OrganizationId};
    use chrono::Utc;

    #[tokio::test]
    async fn adapter_maps_one_agent_execution_intent_and_preserves_operation_replay() {
        let operations = Arc::new(InMemoryOperationRepository::new());
        let scheduler = OperationsAgentExecutionOperationScheduler::new(operations.clone());
        let execution_id = AgentExecutionId::new();
        let operation_id = OperationId::from_uuid(execution_id.as_uuid());
        let organization_id = OrganizationId::new();
        let request = AgentExecutionOperationRequest::new(
            operation_id,
            organization_id,
            execution_id,
            Utc::now(),
        );

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
        assert_eq!(operation.subject.kind(), "agent_execution");
        assert_eq!(operation.subject.id(), execution_id.as_uuid());
        assert_eq!(operation.workflow.name(), AGENT_EXECUTION_WORKFLOW_NAME);
        assert_eq!(
            operation.workflow.version(),
            AGENT_EXECUTION_WORKFLOW_VERSION
        );
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
