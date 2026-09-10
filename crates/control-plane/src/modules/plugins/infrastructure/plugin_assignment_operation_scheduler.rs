use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::plugins::application::{
    IPluginAssignmentOperationScheduler, PluginAssignmentOperationRequest,
    PluginAssignmentOperationScheduleOutcome, PluginAssignmentReconciler,
    PLUGIN_ASSIGNMENT_WORKFLOW_NAME, PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
};
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Sole anti-corruption adapter from Plugins scheduling intent to Operations.
struct OperationsPluginAssignmentOperationScheduler {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsPluginAssignmentOperationScheduler {
    fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IPluginAssignmentOperationScheduler for OperationsPluginAssignmentOperationScheduler {
    async fn schedule(
        &self,
        request: PluginAssignmentOperationRequest,
    ) -> Result<PluginAssignmentOperationScheduleOutcome, RepositoryError> {
        let operation = OperationRequest::new(
            request.operation_id(),
            request.organization_id(),
            OperationSubject::new("plugin_assignment", request.assignment_id().as_uuid())
                .map_err(RepositoryError::Storage)?,
            WorkflowIdentity::new(
                PLUGIN_ASSIGNMENT_WORKFLOW_NAME,
                PLUGIN_ASSIGNMENT_WORKFLOW_VERSION,
            )
            .map_err(RepositoryError::Storage)?,
            json!({
                "organizationId": request.organization_id(),
                "assignmentId": request.assignment_id(),
                "operationId": request.operation_id(),
                "assignmentGeneration": request.assignment_generation(),
            }),
            request.requested_at(),
        );
        self.operations
            .enqueue(operation)
            .await
            .map(|write| PluginAssignmentOperationScheduleOutcome::new(write.replayed))
    }
}

// Preserve the root composition API while keeping Operations types outside
// the Plugins Application layer. New use cases inject IPluginAssignmentOperationScheduler
// directly; the composition root receives this Infrastructure convenience.
impl PluginAssignmentReconciler {
    pub fn new(
        assignments: Arc<dyn IPluginAssignmentRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self::from_operation_scheduler(
            assignments,
            Arc::new(OperationsPluginAssignmentOperationScheduler::new(
                operations,
            )),
        )
    }

    pub fn with_schedule(
        assignments: Arc<dyn IPluginAssignmentRepository>,
        operations: Arc<dyn IOperationRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        Self::with_operation_scheduler_and_schedule(
            assignments,
            Arc::new(OperationsPluginAssignmentOperationScheduler::new(
                operations,
            )),
            interval,
            batch_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{OperationId, OrganizationId, PluginAssignmentId};
    use chrono::Utc;

    #[tokio::test]
    async fn adapter_maps_one_plugin_assignment_intent_and_preserves_operation_replay() {
        let operations = Arc::new(InMemoryOperationRepository::new());
        let scheduler = OperationsPluginAssignmentOperationScheduler::new(operations.clone());
        let assignment_id = PluginAssignmentId::new();
        let operation_id = OperationId::from_uuid(assignment_id.as_uuid());
        let organization_id = OrganizationId::new();
        let request = PluginAssignmentOperationRequest::new(
            operation_id,
            organization_id,
            assignment_id,
            3,
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
        assert_eq!(operation.subject.kind(), "plugin_assignment");
        assert_eq!(operation.subject.id(), assignment_id.as_uuid());
        assert_eq!(operation.workflow.name(), PLUGIN_ASSIGNMENT_WORKFLOW_NAME);
        assert_eq!(
            operation.workflow.version(),
            PLUGIN_ASSIGNMENT_WORKFLOW_VERSION
        );
        assert_eq!(
            operation.input,
            serde_json::json!({
                "organizationId": organization_id,
                "assignmentId": assignment_id,
                "operationId": operation_id,
                "assignmentGeneration": 3,
            })
        );
    }
}
