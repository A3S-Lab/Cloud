use crate::modules::plugins::application::{
    IPluginAssignmentOperationScheduler, PluginAssignmentOperationRequest,
};
use crate::modules::plugins::domain::repositories::IPluginAssignmentRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub const PLUGIN_ASSIGNMENT_WORKFLOW_NAME: &str = "cloud.plugin-assignment";
pub const PLUGIN_ASSIGNMENT_WORKFLOW_VERSION: &str = "1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginAssignmentReconcileReport {
    pub started: usize,
    pub replayed: usize,
    pub failures: Vec<String>,
}

pub struct PluginAssignmentReconciler {
    assignments: Arc<dyn IPluginAssignmentRepository>,
    operation_scheduler: Arc<dyn IPluginAssignmentOperationScheduler>,
    interval: Duration,
    batch_size: usize,
}

impl PluginAssignmentReconciler {
    pub fn from_operation_scheduler(
        assignments: Arc<dyn IPluginAssignmentRepository>,
        operation_scheduler: Arc<dyn IPluginAssignmentOperationScheduler>,
    ) -> Self {
        Self {
            assignments,
            operation_scheduler,
            interval: Duration::from_secs(1),
            batch_size: 100,
        }
    }

    pub fn with_operation_scheduler_and_schedule(
        assignments: Arc<dyn IPluginAssignmentRepository>,
        operation_scheduler: Arc<dyn IPluginAssignmentOperationScheduler>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "plugin assignment reconciliation requires a positive interval and batch size"
                    .into(),
            );
        }
        Ok(Self {
            assignments,
            operation_scheduler,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        limit: usize,
    ) -> Result<PluginAssignmentReconcileReport, RepositoryError> {
        let pending = self
            .assignments
            .pending_operation_starts(limit.max(1))
            .await?;
        let mut report = PluginAssignmentReconcileReport::default();
        for assignment in pending {
            let Some(operation_id) = assignment.current_operation_id else {
                continue;
            };
            let request = PluginAssignmentOperationRequest::new(
                operation_id,
                assignment.organization_id,
                assignment.id,
                assignment.assignment_generation,
                assignment.updated_at,
            );
            match self.operation_scheduler.schedule(request).await {
                Ok(outcome) if outcome.replayed() => report.replayed += 1,
                Ok(_) => report.started += 1,
                Err(error) => report.failures.push(format!(
                    "could not enqueue plugin assignment {} operation: {error}",
                    assignment.id
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
                        return;
                    }
                }
                _ = ticker.tick() => {
                    let _ = self.run_once(self.batch_size).await;
                }
            }
        }
    }
}
