use crate::modules::edge::domain::repositories::IMcpCredentialLifecycleRepository;
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Bounded maintenance for the existing one-time credential delivery store.
/// Credential aggregates and idempotency records remain authoritative after
/// their encrypted recovery material expires.
pub struct McpCredentialDeliveryReceiptSweeper {
    repository: Arc<dyn IMcpCredentialLifecycleRepository>,
    interval: Duration,
    batch_size: usize,
}

impl McpCredentialDeliveryReceiptSweeper {
    pub fn new(
        repository: Arc<dyn IMcpCredentialLifecycleRepository>,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 || batch_size > 10_000 {
            return Err(
                "MCP credential delivery receipt sweeping requires a bounded interval and batch"
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
            .sweep_expired_mcp_credential_delivery_receipts(
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
                            "MCP credential delivery receipt sweep completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "MCP credential delivery receipt sweep failed"
                        ),
                    }
                }
            }
        }
    }
}
