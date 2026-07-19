use crate::modules::fleet::domain::repositories::{
    ILogRetentionRepository, NodeLogCompactionResult,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

pub struct LogCompactionWorker {
    repository: Arc<dyn ILogRetentionRepository>,
    tombstone_retention: chrono::Duration,
    poll_interval: Duration,
    batch_size: usize,
}

impl LogCompactionWorker {
    pub fn new(
        repository: Arc<dyn ILogRetentionRepository>,
        tombstone_retention: Duration,
        poll_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if tombstone_retention.is_zero()
            || poll_interval.is_zero()
            || poll_interval > tombstone_retention
            || batch_size == 0
            || batch_size > 10_000
        {
            return Err("log tombstone compaction policy is invalid".into());
        }
        let tombstone_retention = chrono::Duration::from_std(tombstone_retention)
            .map_err(|_| "log tombstone retention duration exceeds supported bounds")?;
        Ok(Self {
            repository,
            tombstone_retention,
            poll_interval,
            batch_size,
        })
    }

    pub(crate) async fn run_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<NodeLogCompactionResult, RepositoryError> {
        let retained_before = now
            .checked_sub_signed(self.tombstone_retention)
            .ok_or_else(|| RepositoryError::Storage("log compaction cutoff overflowed".into()))?;
        self.repository
            .compact_log_tombstones(retained_before, now, self.batch_size)
            .await
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
                        Ok(result) => {
                            tracing::debug!(
                                compacted_tombstones = result.compacted_tombstones,
                                created_ranges = result.created_ranges,
                                "log tombstone compaction cycle completed"
                            );
                        }
                        Err(error) => {
                            tracing::error!(error = %error, "log tombstone compaction failed");
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "log_compaction_tests.rs"]
mod tests;
