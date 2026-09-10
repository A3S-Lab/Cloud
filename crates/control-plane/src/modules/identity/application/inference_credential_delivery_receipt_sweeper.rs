//! Bounded maintenance for Identity inference credential delivery receipts.

use crate::modules::identity::domain::repositories::IInferenceCredentialLifecycleRepository;
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Bounded maintenance for the existing one-time credential delivery store.
/// Credential aggregates and idempotency records remain authoritative after
/// their encrypted recovery material expires.
pub struct InferenceCredentialDeliveryReceiptSweeper {
    repository: Arc<dyn IInferenceCredentialLifecycleRepository>,
    interval: Duration,
    batch_size: usize,
}

impl InferenceCredentialDeliveryReceiptSweeper {
    pub fn new(
        repository: Arc<dyn IInferenceCredentialLifecycleRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err(
                "inference credential delivery receipt sweeping requires a bounded interval and batch"
                    .into(),
            );
        }
        Ok(Self {
            repository,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(&self, now: DateTime<Utc>) -> Result<usize, RepositoryError> {
        self.repository
            .sweep_expired_inference_credential_delivery_receipts(
                canonical_timestamp(now),
                self.batch_size,
            )
            .await
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
                        Ok(swept) => tracing::debug!(
                            swept,
                            "inference credential delivery receipt sweep completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "inference credential delivery receipt sweep failed"
                        ),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::infrastructure::persistence::InMemoryInferenceCredentialRepository;

    #[tokio::test]
    async fn rejects_unbounded_batch_configuration() {
        let repository = Arc::new(InMemoryInferenceCredentialRepository::default());
        assert!(InferenceCredentialDeliveryReceiptSweeper::new(
            repository as _,
            Duration::from_secs(60),
            0,
        )
        .is_err());
        let repository = Arc::new(InMemoryInferenceCredentialRepository::default());
        assert!(InferenceCredentialDeliveryReceiptSweeper::new(
            repository as _,
            Duration::ZERO,
            100,
        )
        .is_err());
    }

    #[tokio::test]
    async fn run_once_delegates_to_repository_sweep() {
        let repository = Arc::new(InMemoryInferenceCredentialRepository::default());
        let sweeper = InferenceCredentialDeliveryReceiptSweeper::new(
            Arc::clone(&repository) as _,
            Duration::from_secs(60),
            100,
        )
        .unwrap();
        assert_eq!(sweeper.run_once(Utc::now()).await.unwrap(), 0);
    }
}
