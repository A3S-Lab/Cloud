use crate::modules::shared_kernel::domain::{RepositoryError, WorkflowRunId};
use crate::modules::workflow::domain::{
    IWorkflowRunCoordinator, IWorkflowRunRepository, WorkflowRunCoordinationError,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunReconcileFailure {
    pub workflow_run_id: WorkflowRunId,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowRunReconcileReport {
    pub inspected: usize,
    pub projected: usize,
    pub deferred: usize,
    pub failures: Vec<WorkflowRunReconcileFailure>,
}

pub struct WorkflowRunReconciler {
    repository: Arc<dyn IWorkflowRunRepository>,
    coordinator: Arc<dyn IWorkflowRunCoordinator>,
    interval: Duration,
    batch_size: usize,
}

impl WorkflowRunReconciler {
    pub fn new(
        repository: Arc<dyn IWorkflowRunRepository>,
        coordinator: Arc<dyn IWorkflowRunCoordinator>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "WorkflowRun reconciliation requires a positive interval and batch size".into(),
            );
        }
        Ok(Self {
            repository,
            coordinator,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        limit: usize,
    ) -> Result<WorkflowRunReconcileReport, RepositoryError> {
        let pending = self.repository.pending_reconciliation(limit.max(1)).await?;
        let mut report = WorkflowRunReconcileReport {
            inspected: pending.len(),
            ..WorkflowRunReconcileReport::default()
        };
        for record in pending {
            let workflow_run_id = record.run.id;
            let expected_version = record.run.aggregate_version;
            let projected = match self
                .coordinator
                .reconcile(&record, chrono::Utc::now())
                .await
            {
                Ok(Some(projected)) => projected,
                Ok(None) => continue,
                Err(WorkflowRunCoordinationError::Deferred(_)) => {
                    report.deferred += 1;
                    continue;
                }
                Err(error) => {
                    report.failures.push(WorkflowRunReconcileFailure {
                        workflow_run_id,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            match self
                .repository
                .save_projection(projected, expected_version)
                .await
            {
                Ok(_) => report.projected += 1,
                Err(error) => report.failures.push(WorkflowRunReconcileFailure {
                    workflow_run_id,
                    error: error.to_string(),
                }),
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
                            for failure in report.failures {
                                tracing::warn!(
                                    workflow_run_id = %failure.workflow_run_id,
                                    error = %failure.error,
                                    "WorkflowRun reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "WorkflowRun reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }
}
