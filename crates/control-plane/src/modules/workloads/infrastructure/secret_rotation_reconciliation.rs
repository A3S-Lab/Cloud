use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::workloads::domain::repositories::{
    ISecretRotationRestartRepository, SecretRotationCompletion,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRotationRestartFailure {
    pub event_id: Uuid,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretRotationRestartReport {
    pub inspected_rotations: usize,
    pub scheduled_restarts: usize,
    pub completed_rotations: usize,
    pub deferred_rotations: usize,
    pub superseded_rotations: usize,
    pub unavailable_rotations: usize,
    pub failures: Vec<SecretRotationRestartFailure>,
}

pub struct SecretRotationRestartReconciler {
    repository: Arc<dyn ISecretRotationRestartRepository>,
    interval: Duration,
    rotation_batch_size: usize,
    workload_batch_size: usize,
}

impl SecretRotationRestartReconciler {
    pub fn new(
        repository: Arc<dyn ISecretRotationRestartRepository>,
        interval: Duration,
        rotation_batch_size: usize,
        workload_batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || rotation_batch_size == 0 || workload_batch_size == 0 {
            return Err(
                "Secret rotation restart reconciliation requires positive timing and batch sizes"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            interval,
            rotation_batch_size,
            workload_batch_size,
        })
    }

    pub async fn run_once(
        &self,
        reconciled_at: DateTime<Utc>,
    ) -> Result<SecretRotationRestartReport, RepositoryError> {
        let rotations = self
            .repository
            .pending_secret_rotations(self.rotation_batch_size)
            .await?;
        let mut report = SecretRotationRestartReport {
            inspected_rotations: rotations.len(),
            ..SecretRotationRestartReport::default()
        };
        for rotation in rotations {
            let event_id = rotation.event_id;
            match self
                .repository
                .reconcile_secret_rotation(rotation, self.workload_batch_size, reconciled_at)
                .await
            {
                Ok(reconciliation) => {
                    report.scheduled_restarts += reconciliation.scheduled.len();
                    match reconciliation.completion {
                        Some(SecretRotationCompletion::Scheduled)
                        | Some(SecretRotationCompletion::NoTargets) => {
                            report.completed_rotations += 1;
                        }
                        Some(SecretRotationCompletion::Superseded) => {
                            report.superseded_rotations += 1;
                        }
                        Some(SecretRotationCompletion::Unavailable) => {
                            report.unavailable_rotations += 1;
                        }
                        None if reconciliation.scheduled.is_empty() => {
                            report.deferred_rotations += 1;
                        }
                        None => {}
                    }
                }
                Err(error) => report.failures.push(SecretRotationRestartFailure {
                    event_id,
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
                    match self.run_once(Utc::now()).await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    secret_rotation_event_id = %failure.event_id,
                                    error = %failure.error,
                                    "Secret rotation restart reconciliation failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "Secret rotation restart scan failed"
                        ),
                    }
                }
            }
        }
    }
}
