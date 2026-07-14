use crate::modules::integration_events::domain::repositories::IOutboxRepository;
use crate::modules::integration_events::domain::services::IEventPublisher;
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct OutboxRelayConfig {
    pub batch_size: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub publish_timeout: Duration,
    pub initial_backoff: Duration,
    pub maximum_backoff: Duration,
}

impl OutboxRelayConfig {
    pub fn validate(self) -> Result<Self, String> {
        if self.batch_size == 0
            || self.poll_interval.is_zero()
            || self.lease_duration <= self.publish_timeout
            || self.publish_timeout.is_zero()
            || self.initial_backoff.is_zero()
            || self.maximum_backoff < self.initial_backoff
        {
            return Err("outbox relay requires a positive batch and timings, a lease longer than publish timeout, and ordered backoff bounds".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRelayFailure {
    pub event_id: Uuid,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboxRelayReport {
    pub claimed: usize,
    pub published: usize,
    pub failures: Vec<OutboxRelayFailure>,
}

pub struct OutboxRelay {
    owner: Uuid,
    repository: Arc<dyn IOutboxRepository>,
    publisher: Arc<dyn IEventPublisher>,
    config: OutboxRelayConfig,
}

impl OutboxRelay {
    pub fn new(
        repository: Arc<dyn IOutboxRepository>,
        publisher: Arc<dyn IEventPublisher>,
        config: OutboxRelayConfig,
    ) -> Result<Self, String> {
        Ok(Self {
            owner: Uuid::new_v4(),
            repository,
            publisher,
            config: config.validate()?,
        })
    }

    pub async fn run_once(&self) -> Result<OutboxRelayReport, RepositoryError> {
        let messages = self
            .repository
            .claim(
                self.owner,
                self.config.batch_size,
                self.config.lease_duration,
            )
            .await?;
        let mut report = OutboxRelayReport {
            claimed: messages.len(),
            ..OutboxRelayReport::default()
        };
        for message in messages {
            let event_id = message.event_id;
            let publish = tokio::time::timeout(
                self.config.publish_timeout,
                self.publisher.publish(&message),
            )
            .await;
            let failure = match publish {
                Ok(Ok(())) => match self
                    .repository
                    .mark_published(event_id, self.owner, Utc::now())
                    .await
                {
                    Ok(()) => {
                        report.published += 1;
                        continue;
                    }
                    Err(error) => format!(
                        "event was published but its outbox acknowledgement failed: {error}"
                    ),
                },
                Ok(Err(error)) => error.to_string(),
                Err(_) => format!(
                    "integration event publish timed out after {} ms",
                    self.config.publish_timeout.as_millis()
                ),
            };
            if let Err(mark_error) = self
                .repository
                .mark_failed(
                    event_id,
                    self.owner,
                    &failure,
                    retry_delay(&self.config, message.delivery_attempts),
                )
                .await
            {
                report.failures.push(OutboxRelayFailure {
                    event_id,
                    error: format!("{failure}; could not schedule retry: {mark_error}"),
                });
            } else {
                report.failures.push(OutboxRelayFailure {
                    event_id,
                    error: failure,
                });
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once().await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    event_id = %failure.event_id,
                                    error = %failure.error,
                                    "outbox delivery failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(error = %error, "outbox claim failed"),
                    }
                }
            }
        }
    }
}

fn retry_delay(config: &OutboxRelayConfig, attempts: u32) -> Duration {
    let exponent = attempts.saturating_sub(1).min(20);
    config
        .initial_backoff
        .saturating_mul(1_u32 << exponent)
        .min(config.maximum_backoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backoff_is_bounded_and_independent() {
        let config = OutboxRelayConfig {
            batch_size: 10,
            poll_interval: Duration::from_millis(100),
            lease_duration: Duration::from_secs(10),
            publish_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(250),
            maximum_backoff: Duration::from_secs(2),
        };
        assert_eq!(retry_delay(&config, 1), Duration::from_millis(250));
        assert_eq!(retry_delay(&config, 2), Duration::from_millis(500));
        assert_eq!(retry_delay(&config, 20), Duration::from_secs(2));
    }
}
