use super::commands::reconcile_operations::{
    ReconcileOperationsHandler, ReconcileOperationsReport,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub struct OperationReconciler {
    handler: Arc<ReconcileOperationsHandler>,
    interval: Duration,
    batch_size: usize,
}

impl OperationReconciler {
    pub fn new(
        handler: Arc<ReconcileOperationsHandler>,
        interval: Duration,
        batch_size: usize,
    ) -> Self {
        Self {
            handler,
            interval,
            batch_size: batch_size.max(1),
        }
    }

    pub async fn run_once(&self) -> Result<ReconcileOperationsReport, RepositoryError> {
        self.handler.execute(self.batch_size).await
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once().await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    operation_id = %failure.operation_id,
                                    error = %failure.error,
                                    "operation reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "operation reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}
