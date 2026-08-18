use super::commands::reconcile_operations::{
    ReconcileOperationsHandler, ReconcileOperationsReport,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use std::sync::Arc;

/// Clockless projection step invoked by the infrastructure-owned Flow
/// coordinator. Operations owns the user-visible projection, not a timer,
/// queue, worker lifecycle, or retry loop.
pub struct OperationReconciler {
    handler: Arc<ReconcileOperationsHandler>,
    batch_size: usize,
}

impl OperationReconciler {
    pub fn new(handler: Arc<ReconcileOperationsHandler>, batch_size: usize) -> Self {
        Self {
            handler,
            batch_size: batch_size.max(1),
        }
    }

    pub async fn run_once(&self) -> Result<ReconcileOperationsReport, RepositoryError> {
        self.handler.execute(self.batch_size).await
    }
}
