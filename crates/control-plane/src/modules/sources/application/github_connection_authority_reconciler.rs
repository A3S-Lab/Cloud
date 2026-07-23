use crate::modules::shared_kernel::domain::RepositoryError;
use crate::modules::sources::domain::{
    GithubConnection, GithubConnectionAuthorityError, GithubConnectionAuthorityRequest,
    GithubConnectionReconciled, GithubInstallationAuthorityError,
    GithubInstallationAuthorityRequest, GithubProviderCheckError,
    IGithubConnectionAuthorityService, IGithubConnectionRepository,
    IGithubInstallationAuthorityProvider, PersistGithubProviderReconciliation,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GithubConnectionAuthorityReconcileReport {
    pub scanned: usize,
    pub checked: usize,
    pub lifecycle_changes: usize,
    pub failures: Vec<String>,
}

#[derive(Clone)]
pub struct GithubConnectionAuthorityReconciler {
    connections: Arc<dyn IGithubConnectionRepository>,
    provider: Arc<dyn IGithubInstallationAuthorityProvider>,
    scan_interval: Duration,
    poll_interval: ChronoDuration,
    retry_initial: Duration,
    retry_maximum: Duration,
    batch_size: usize,
}

impl GithubConnectionAuthorityReconciler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connections: Arc<dyn IGithubConnectionRepository>,
        provider: Arc<dyn IGithubInstallationAuthorityProvider>,
        scan_interval: Duration,
        poll_interval: Duration,
        retry_initial: Duration,
        retry_maximum: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if scan_interval.is_zero()
            || poll_interval.is_zero()
            || retry_initial.is_zero()
            || retry_maximum < retry_initial
            || batch_size == 0
        {
            return Err(
                "GitHub authority reconciliation requires positive ordered schedules and a batch"
                    .into(),
            );
        }
        let poll_interval = ChronoDuration::from_std(poll_interval)
            .map_err(|_| "GitHub authority poll interval exceeds the supported range")?;
        Ok(Self {
            connections,
            provider,
            scan_interval,
            poll_interval,
            retry_initial,
            retry_maximum,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        checked_at: DateTime<Utc>,
        limit: usize,
    ) -> Result<GithubConnectionAuthorityReconcileReport, RepositoryError> {
        let candidates = self
            .connections
            .find_provider_check_candidates(checked_at, limit.max(1))
            .await?;
        let mut report = GithubConnectionAuthorityReconcileReport {
            scanned: candidates.len(),
            ..GithubConnectionAuthorityReconcileReport::default()
        };
        for connection in candidates {
            let connection_id = connection.id;
            match self.refresh(connection, checked_at).await {
                Ok(outcome) => {
                    report.checked += 1;
                    if outcome.lifecycle_changed {
                        report.lifecycle_changes += 1;
                    }
                }
                Err(RefreshError::Conflict) => report.failures.push(format!(
                    "GitHub connection {connection_id} changed during provider reconciliation"
                )),
                Err(RefreshError::NotFound) => report.failures.push(format!(
                    "GitHub connection {connection_id} disappeared during provider reconciliation"
                )),
                Err(RefreshError::Unavailable) => report.failures.push(format!(
                    "GitHub connection {connection_id} provider authority is unavailable"
                )),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.scan_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(Utc::now(), self.batch_size).await {
                        Ok(report) => {
                            for error in report.failures {
                                tracing::warn!(error = %error, "GitHub authority reconciliation failed");
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "GitHub authority reconciliation scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn refresh(
        &self,
        mut connection: GithubConnection,
        checked_at: DateTime<Utc>,
    ) -> Result<RefreshOutcome, RefreshError> {
        if !connection.needs_provider_check() {
            return Err(RefreshError::NotFound);
        }
        let expected_version = connection.aggregate_version;
        let authority = self
            .provider
            .inspect(GithubInstallationAuthorityRequest {
                installation_id: connection.installation_id,
                checked_at,
            })
            .await;
        match authority {
            Ok(authority) => {
                let next_check_at = checked_at
                    .checked_add_signed(self.poll_interval)
                    .ok_or(RefreshError::Unavailable)?;
                let reconciliation = connection
                    .reconcile_provider_authority(authority, checked_at, next_check_at)
                    .map_err(|_| RefreshError::Unavailable)?;
                let event = if reconciliation.lifecycle_changed {
                    Some(
                        GithubConnectionReconciled::envelope(&connection, Uuid::now_v7())
                            .map_err(|_| RefreshError::Unavailable)?,
                    )
                } else {
                    None
                };
                let connection = self
                    .connections
                    .save_provider_reconciliation(PersistGithubProviderReconciliation {
                        connection,
                        expected_version,
                        event,
                    })
                    .await
                    .map_err(map_repository_error)?;
                Ok(RefreshOutcome {
                    lifecycle_changed: reconciliation.lifecycle_changed,
                    connection,
                })
            }
            Err(error) => {
                let error_kind = provider_error_kind(&error);
                let retry_at = checked_at
                    .checked_add_signed(
                        ChronoDuration::from_std(
                            self.retry_delay(connection.provider_check_failures),
                        )
                        .map_err(|_| RefreshError::Unavailable)?,
                    )
                    .ok_or(RefreshError::Unavailable)?;
                connection
                    .record_provider_check_failure(error_kind, checked_at, retry_at)
                    .map_err(|_| RefreshError::Unavailable)?;
                self.connections
                    .save_provider_reconciliation(PersistGithubProviderReconciliation {
                        connection,
                        expected_version,
                        event: None,
                    })
                    .await
                    .map_err(map_repository_error)?;
                Err(RefreshError::Unavailable)
            }
        }
    }

    fn retry_delay(&self, prior_failures: u32) -> Duration {
        let mut delay = self.retry_initial;
        for _ in 0..prior_failures.min(31) {
            delay = delay
                .checked_mul(2)
                .unwrap_or(self.retry_maximum)
                .min(self.retry_maximum);
            if delay == self.retry_maximum {
                break;
            }
        }
        delay
    }
}

#[async_trait]
impl IGithubConnectionAuthorityService for GithubConnectionAuthorityReconciler {
    async fn require_current(
        &self,
        request: GithubConnectionAuthorityRequest,
    ) -> Result<GithubConnection, GithubConnectionAuthorityError> {
        for attempt in 0..2 {
            let connection = self
                .connections
                .find(request.organization_id)
                .await
                .map_err(|_| GithubConnectionAuthorityError::Unavailable)?
                .filter(|connection| {
                    connection.id == request.connection_id && connection.blocks_reconnection()
                })
                .ok_or(GithubConnectionAuthorityError::NotFound)?;
            match self.refresh(connection, request.checked_at).await {
                Ok(outcome) if outcome.connection.is_authoritative() => {
                    return Ok(outcome.connection)
                }
                Ok(_) => return Err(GithubConnectionAuthorityError::Forbidden),
                Err(RefreshError::Conflict) if attempt == 0 => continue,
                Err(RefreshError::NotFound) => {
                    return Err(GithubConnectionAuthorityError::NotFound)
                }
                Err(RefreshError::Conflict | RefreshError::Unavailable) => {
                    return Err(GithubConnectionAuthorityError::Unavailable)
                }
            }
        }
        Err(GithubConnectionAuthorityError::Unavailable)
    }
}

struct RefreshOutcome {
    connection: GithubConnection,
    lifecycle_changed: bool,
}

enum RefreshError {
    NotFound,
    Conflict,
    Unavailable,
}

fn map_repository_error(error: RepositoryError) -> RefreshError {
    match error {
        RepositoryError::NotFound => RefreshError::NotFound,
        RepositoryError::Conflict(_) | RepositoryError::IdempotencyConflict => {
            RefreshError::Conflict
        }
        RepositoryError::Storage(_) => RefreshError::Unavailable,
    }
}

fn provider_error_kind(error: &GithubInstallationAuthorityError) -> GithubProviderCheckError {
    match error {
        GithubInstallationAuthorityError::NotConfigured => GithubProviderCheckError::NotConfigured,
        GithubInstallationAuthorityError::Unavailable => GithubProviderCheckError::Unavailable,
        GithubInstallationAuthorityError::Protocol(_) => GithubProviderCheckError::Protocol,
    }
}

#[cfg(test)]
mod tests;
