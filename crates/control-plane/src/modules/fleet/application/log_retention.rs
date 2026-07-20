use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, NodeLogRetentionTarget,
};
use crate::modules::fleet::domain::services::ILogChunkStore;
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogRetentionFailure {
    pub(crate) target: NodeLogRetentionTarget,
    pub(crate) error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LogRetentionReport {
    pub(crate) inspected: usize,
    pub(crate) retained: usize,
    pub(crate) concurrently_retained: usize,
    pub(crate) failures: Vec<LogRetentionFailure>,
}

pub struct LogRetentionWorker {
    repository: Arc<dyn ILogRetentionRepository>,
    objects: Arc<dyn ILogChunkStore>,
    retention: chrono::Duration,
    poll_interval: Duration,
    batch_size: usize,
}

impl LogRetentionWorker {
    pub fn new(
        repository: Arc<dyn ILogRetentionRepository>,
        objects: Arc<dyn ILogChunkStore>,
        retention: Duration,
        poll_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if retention.is_zero()
            || poll_interval.is_zero()
            || poll_interval > retention
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err("log retention policy is invalid".into());
        }
        let retention = chrono::Duration::from_std(retention)
            .map_err(|_| "log retention duration exceeds supported bounds")?;
        Ok(Self {
            repository,
            objects,
            retention,
            poll_interval,
            batch_size,
        })
    }

    pub(crate) async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<LogRetentionReport, RepositoryError> {
        let received_before = now
            .checked_sub_signed(self.retention)
            .ok_or_else(|| RepositoryError::Storage("log retention cutoff overflowed".into()))?;
        let targets = self
            .repository
            .list_log_chunks_for_retention(received_before, self.batch_size)
            .await?;
        let mut report = LogRetentionReport {
            inspected: targets.len(),
            ..LogRetentionReport::default()
        };
        for target in targets {
            if let Err(error) = target.validate() {
                report.failures.push(LogRetentionFailure { target, error });
                continue;
            }
            if let Err(error) = self.objects.remove(&target.object_key).await {
                report.failures.push(LogRetentionFailure {
                    target,
                    error: error.to_string(),
                });
                continue;
            }
            match self.repository.mark_log_chunk_retained(&target, now).await {
                Ok(true) => report.retained += 1,
                Ok(false) => report.concurrently_retained += 1,
                Err(error) => report.failures.push(LogRetentionFailure {
                    target,
                    error: error.to_string(),
                }),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.poll_interval);
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
                            for failure in &report.failures {
                                tracing::warn!(
                                    node_id = %failure.target.node_id,
                                    unit_id = %failure.target.unit_id,
                                    generation = failure.target.generation,
                                    sequence = failure.target.sequence,
                                    error = %failure.error,
                                    "log retention target will retry"
                                );
                            }
                            tracing::debug!(
                                inspected = report.inspected,
                                retained = report.retained,
                                concurrently_retained = report.concurrently_retained,
                                failures = report.failures.len(),
                                "log retention cycle completed"
                            );
                        }
                        Err(error) => {
                            tracing::error!(error = %error, "log retention scan failed");
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "log_retention_tests.rs"]
mod tests;
