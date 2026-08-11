use crate::modules::workflow::domain::WorkflowRunRecord;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkflowRunCoordinationError {
    #[error("WorkflowRun Flow state is not available yet: {0}")]
    Deferred(String),
    #[error("WorkflowRun Flow coordination failed: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait IWorkflowRunCoordinator: Send + Sync {
    async fn reconcile(
        &self,
        record: &WorkflowRunRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowRunCoordinationError>;
}
