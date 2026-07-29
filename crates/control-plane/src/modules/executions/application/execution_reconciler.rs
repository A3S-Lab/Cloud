use crate::modules::executions::domain::IExecutionRepository;
use crate::modules::operations::domain::entities::OperationRequest;
use crate::modules::operations::domain::repositories::IOperationRepository;
use crate::modules::operations::domain::value_objects::{OperationSubject, WorkflowIdentity};
use crate::modules::shared_kernel::domain::RepositoryError;
use serde_json::json;
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
    operations: Arc<dyn IOperationRepository>,
    interval: Duration,
    batch_size: usize,
}

impl ExecutionReconciler {
    pub fn new(
        executions: Arc<dyn IExecutionRepository>,
        operations: Arc<dyn IOperationRepository>,
    ) -> Self {
        Self {
            executions,
            operations,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_schedule(
        executions: Arc<dyn IExecutionRepository>,
        operations: Arc<dyn IOperationRepository>,
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
            operations,
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
            let operation = OperationRequest::new(
                execution.operation_id,
                execution.organization_id,
                OperationSubject::new("execution", execution.id.as_uuid())
                    .map_err(RepositoryError::Storage)?,
                WorkflowIdentity::new(EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION)
                    .map_err(RepositoryError::Storage)?,
                json!({
                    "organizationId": execution.organization_id,
                    "executionId": execution.id,
                }),
                execution.requested_at,
            );
            match self.operations.enqueue(operation).await {
                Ok(write) if write.replayed => report.replayed += 1,
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
