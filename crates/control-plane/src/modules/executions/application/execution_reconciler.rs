use crate::modules::executions::application::{
    ExecutionOperationRequest, IExecutionOperationScheduler,
};
use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub const EXECUTION_WORKFLOW_NAME: &str = "cloud.execution";
pub const EXECUTION_WORKFLOW_VERSION: &str = "1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionReconcileReport {
    pub started: usize,
    pub replayed: usize,
    pub failures: Vec<String>,
}

pub struct ExecutionReconciler {
    executions: Arc<dyn IExecutionRepository>,
    operation_scheduler: Arc<dyn IExecutionOperationScheduler>,
    interval: Duration,
    batch_size: usize,
}

impl ExecutionReconciler {
    pub fn from_operation_scheduler(
        executions: Arc<dyn IExecutionRepository>,
        operation_scheduler: Arc<dyn IExecutionOperationScheduler>,
    ) -> Self {
        Self {
            executions,
            operation_scheduler,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_operation_scheduler_and_schedule(
        executions: Arc<dyn IExecutionRepository>,
        operation_scheduler: Arc<dyn IExecutionOperationScheduler>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "execution reconciliation requires a positive interval and batch size".into(),
            );
        }
        Ok(Self {
            executions,
            operation_scheduler,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        limit: usize,
    ) -> Result<ExecutionReconcileReport, RepositoryError> {
        let pending = self
            .executions
            .pending_operation_starts(limit.max(1))
            .await?;
        let mut report = ExecutionReconcileReport::default();
        for execution in pending {
            let request = ExecutionOperationRequest::new(
                execution.operation_id,
                execution.organization_id,
                execution.id,
                execution.requested_at,
            );
            match self.operation_scheduler.schedule(request).await {
                Ok(outcome) if outcome.replayed() => report.replayed += 1,
                Ok(_) => report.started += 1,
                Err(error) => report.failures.push(format!(
                    "could not enqueue execution {} operation: {error}",
                    execution.id
                )),
            }
        }
        Ok(report)
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
                    match self.run_once(self.batch_size).await {
                        Ok(report) => {
                            for error in report.failures {
                                tracing::warn!(error = %error, "execution reconciliation failed");
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "execution reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}
