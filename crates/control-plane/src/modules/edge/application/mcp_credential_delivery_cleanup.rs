use crate::modules::edge::domain::repositories::{
    IMcpCredentialLifecycleRepository, MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, RepositoryError};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Periodically removes expired encrypted credential recovery material.
///
/// Idempotency references and credential aggregates remain durable, so a
/// purged issuance or rotation can never mint a replacement secret.
pub struct McpCredentialDeliveryCleanupWorker {
    repository: Arc<dyn IMcpCredentialLifecycleRepository>,
    poll_interval: Duration,
    batch_size: usize,
}

impl McpCredentialDeliveryCleanupWorker {
    pub fn new(
        repository: Arc<dyn IMcpCredentialLifecycleRepository>,
        poll_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if poll_interval.is_zero()
            || batch_size == 0
            || batch_size > MAX_MCP_CREDENTIAL_DELIVERY_PURGE_BATCH
        {
            return Err("MCP credential delivery cleanup policy is invalid".into());
        }
        Ok(Self {
            repository,
            poll_interval,
            batch_size,
        })
    }

    pub(crate) async fn run_once(
        &self,
        observed_at: DateTime<Utc>,
    ) -> Result<usize, RepositoryError> {
        self.repository
            .purge_expired_mcp_credential_deliveries(
                canonical_timestamp(observed_at),
                self.batch_size,
            )
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
                        Ok(purged) => tracing::debug!(
                            purged,
                            "MCP credential delivery cleanup cycle completed"
                        ),
                        Err(error) => tracing::error!(
                            error = %error,
                            "MCP credential delivery cleanup scan failed"
                        ),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "mcp_credential_delivery_cleanup_tests.rs"]
mod tests;
