use crate::modules::artifacts::application::{
    BuildOperationRequest, BuildOperationScheduleOutcome, BuildRunReconciler,
    IBuildOperationScheduler, BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION,
};
use crate::modules::artifacts::domain::IBuildRunRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

/// Sole anti-corruption adapter from Artifacts scheduling intent to Operations.
struct OperationsBuildOperationScheduler {
    operations: Arc<dyn IOperationRepository>,
}

impl OperationsBuildOperationScheduler {
    fn new(operations: Arc<dyn IOperationRepository>) -> Self {
        Self { operations }
    }
}

#[async_trait]
impl IBuildOperationScheduler for OperationsBuildOperationScheduler {
    async fn schedule(
        &self,
        request: BuildOperationRequest,
    ) -> Result<BuildOperationScheduleOutcome, RepositoryError> {
        let operation = OperationRequest::new(
            request.operation_id(),
            request.organization_id(),
            OperationSubject::new("build_run", request.build_run_id().as_uuid())
                .map_err(RepositoryError::Storage)?,
            WorkflowIdentity::new(BUILD_WORKFLOW_NAME, BUILD_WORKFLOW_VERSION)
                .map_err(RepositoryError::Storage)?,
            json!({
                "organizationId": request.organization_id(),
                "buildRunId": request.build_run_id(),
            }),
            request.requested_at(),
        );
        self.operations
            .enqueue(operation)
            .await
            .map(|write| BuildOperationScheduleOutcome::new(write.replayed))
    }
}

// Preserve the root composition API while keeping Operations types outside
// the Artifacts Application layer. New use cases inject IBuildOperationScheduler
// directly; the composition root receives this Infrastructure convenience.
impl BuildRunReconciler {
    pub fn new(
        builds: Arc<dyn IBuildRunRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self::from_operation_scheduler(
            builds,
            Arc::new(OperationsBuildOperationScheduler::new(operations)),
        )
    }

    pub fn with_schedule(
        builds: Arc<dyn IBuildRunRepository>,
        operations: Arc<dyn IOperationRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        Self::with_operation_scheduler_and_schedule(
            builds,
            Arc::new(OperationsBuildOperationScheduler::new(operations)),
            interval,
            batch_size,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::operations::InMemoryOperationRepository;
    use crate::modules::shared_kernel::domain::{BuildRunId, OperationId, OrganizationId};
    use chrono::Utc;

    #[tokio::test]
    async fn adapter_maps_one_build_intent_and_preserves_operation_replay() {
        let operations = Arc::new(InMemoryOperationRepository::new());
        let scheduler = OperationsBuildOperationScheduler::new(operations.clone());
        let build_run_id = BuildRunId::new();
        let operation_id = OperationId::from_uuid(build_run_id.as_uuid());
        let organization_id = OrganizationId::new();
        let request =
            BuildOperationRequest::new(operation_id, organization_id, build_run_id, Utc::now());

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
        assert_eq!(operation.subject.kind(), "build_run");
        assert_eq!(operation.subject.id(), build_run_id.as_uuid());
        assert_eq!(operation.workflow.name(), BUILD_WORKFLOW_NAME);
        assert_eq!(operation.workflow.version(), BUILD_WORKFLOW_VERSION);
        assert_eq!(
            operation.input["organizationId"],
            organization_id.to_string()
        );
        assert_eq!(operation.input["buildRunId"], build_run_id.to_string());
    }
}
