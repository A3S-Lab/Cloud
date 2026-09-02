use super::ports::{EventPublishError, IEventPublisher, IIntegrationEventProjector};
use crate::modules::integration_events::domain::repositories::IOutboxRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct OutboxRelayConfig {
    /// Maximum messages processed during one polling turn. Each message is claimed immediately
    /// before delivery so this bound never extends the lease wait of another claimed message.
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
    projectors: Vec<Arc<dyn IIntegrationEventProjector>>,
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
            projectors: Vec::new(),
            config: config.validate()?,
        })
    }

    pub fn with_projector(mut self, projector: Arc<dyn IIntegrationEventProjector>) -> Self {
        self.projectors.push(projector);
        self
    }

    pub async fn run_once(&self) -> Result<OutboxRelayReport, RepositoryError> {
        let mut report = OutboxRelayReport::default();
        while report.claimed < self.config.batch_size {
            let mut messages = self
                .repository
                .claim(self.owner, 1, self.config.lease_duration)
                .await?;
            if messages.len() > 1 {
                return Err(RepositoryError::Storage(
                    "Outbox repository returned more messages than the requested claim limit"
                        .into(),
                ));
            }
            let Some(message) = messages.pop() else {
                break;
            };
            report.claimed += 1;
            match self.deliver_claimed(message).await {
                Ok(()) => report.published += 1,
                Err(failure) => {
                    report.failures.push(failure);
                    // A failed delivery establishes provider backpressure and must not reclaim the
                    // same fact after a short retry delay during this polling turn.
                    break;
                }
            }
        }
        Ok(report)
    }

    async fn deliver_claimed(
        &self,
        message: crate::modules::integration_events::domain::entities::OutboxMessage,
    ) -> Result<(), OutboxRelayFailure> {
        let event_id = message.event_id;
        let publish = tokio::time::timeout(self.config.publish_timeout, async {
            message.domain_event().map_err(|error| {
                EventPublishError::new(format!("committed integration event is invalid: {error}"))
            })?;
            for projector in &self.projectors {
                projector.project(&message).await.map_err(|error| {
                    EventPublishError::new(format!("integration event projection failed: {error}"))
                })?;
            }
            self.publisher.publish(&message).await
        })
        .await;
        let failure = match publish {
            Ok(Ok(())) => match self
                .repository
                .mark_published(event_id, self.owner, Utc::now())
                .await
            {
                Ok(()) => return Ok(()),
                Err(error) => {
                    format!("event was published but its outbox acknowledgement failed: {error}")
                }
            },
            Ok(Err(error)) => error.to_string(),
            Err(_) => format!(
                "integration event publish timed out after {} ms",
                self.config.publish_timeout.as_millis()
            ),
        };
        let error = match self
            .repository
            .mark_failed(
                event_id,
                self.owner,
                &failure,
                retry_delay(&self.config, message.delivery_attempts),
            )
            .await
        {
            Ok(()) => failure,
            Err(mark_error) => format!("{failure}; could not schedule retry: {mark_error}"),
        };
        Err(OutboxRelayFailure { event_id, error })
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
    use crate::modules::integration_events::domain::entities::OutboxMessage;
    use crate::modules::shared_kernel::domain::{InstallationId, ScopeContext};
    use async_trait::async_trait;
    use chrono::DateTime;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    struct RecordingOutboxRepository {
        state: Mutex<RecordingOutboxState>,
    }

    struct RecordingOutboxState {
        ready: VecDeque<OutboxMessage>,
        outstanding: Option<Uuid>,
        claim_limits: Vec<usize>,
        published: Vec<Uuid>,
        failed: Vec<Uuid>,
    }

    impl RecordingOutboxRepository {
        fn new(messages: impl IntoIterator<Item = OutboxMessage>) -> Self {
            Self {
                state: Mutex::new(RecordingOutboxState {
                    ready: messages.into_iter().collect(),
                    outstanding: None,
                    claim_limits: Vec::new(),
                    published: Vec::new(),
                    failed: Vec::new(),
                }),
            }
        }
    }

    #[async_trait]
    impl IOutboxRepository for RecordingOutboxRepository {
        async fn claim(
            &self,
            _owner: Uuid,
            limit: usize,
            _lease_duration: Duration,
        ) -> Result<Vec<OutboxMessage>, RepositoryError> {
            let mut state = self.state.lock().await;
            if state.outstanding.is_some() {
                return Err(RepositoryError::Conflict(
                    "another message was claimed before the active delivery settled".into(),
                ));
            }
            state.claim_limits.push(limit);
            let Some(message) = state.ready.pop_front() else {
                return Ok(Vec::new());
            };
            state.outstanding = Some(message.event_id);
            Ok(vec![message])
        }

        async fn mark_published(
            &self,
            event_id: Uuid,
            _owner: Uuid,
            _published_at: DateTime<Utc>,
        ) -> Result<(), RepositoryError> {
            let mut state = self.state.lock().await;
            if state.outstanding != Some(event_id) {
                return Err(RepositoryError::Conflict(
                    "published event does not own the active claim".into(),
                ));
            }
            state.outstanding = None;
            state.published.push(event_id);
            Ok(())
        }

        async fn mark_failed(
            &self,
            event_id: Uuid,
            _owner: Uuid,
            _error: &str,
            _retry_after: Duration,
        ) -> Result<(), RepositoryError> {
            let mut state = self.state.lock().await;
            if state.outstanding != Some(event_id) {
                return Err(RepositoryError::Conflict(
                    "failed event does not own the active claim".into(),
                ));
            }
            state.outstanding = None;
            state.failed.push(event_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct CountingPublisher {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IEventPublisher for CountingPublisher {
        async fn publish(&self, _message: &OutboxMessage) -> Result<(), EventPublishError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn health(&self) -> Result<bool, EventPublishError> {
            Ok(true)
        }
    }

    fn message() -> OutboxMessage {
        OutboxMessage {
            event_id: Uuid::now_v7(),
            event_key: "identity.platform-role.changed".into(),
            schema_version: 1,
            scope: ScopeContext::installation(InstallationId::new()).expect("Installation scope"),
            aggregate_id: Uuid::now_v7(),
            aggregate_version: 1,
            occurred_at: Utc::now(),
            correlation_id: Uuid::now_v7(),
            causation_id: None,
            payload: serde_json::json!({"changed": true}),
            delivery_attempts: 1,
        }
    }

    fn config(batch_size: usize) -> OutboxRelayConfig {
        OutboxRelayConfig {
            batch_size,
            poll_interval: Duration::from_millis(100),
            lease_duration: Duration::from_secs(10),
            publish_timeout: Duration::from_secs(2),
            initial_backoff: Duration::from_millis(250),
            maximum_backoff: Duration::from_secs(2),
        }
    }

    #[test]
    fn retry_backoff_is_bounded_and_independent() {
        let config = config(10);
        assert_eq!(retry_delay(&config, 1), Duration::from_millis(250));
        assert_eq!(retry_delay(&config, 2), Duration::from_millis(500));
        assert_eq!(retry_delay(&config, 20), Duration::from_secs(2));
    }

    #[tokio::test]
    async fn polling_budget_claims_each_fact_only_when_its_delivery_can_start() {
        let repository = Arc::new(RecordingOutboxRepository::new([
            message(),
            message(),
            message(),
        ]));
        let publisher = Arc::new(CountingPublisher::default());
        let relay = OutboxRelay::new(repository.clone(), publisher.clone(), config(10))
            .expect("Outbox Relay");

        let report = relay.run_once().await.expect("relay turn");

        assert_eq!(report.claimed, 3);
        assert_eq!(report.published, 3);
        assert!(report.failures.is_empty());
        assert_eq!(publisher.calls.load(Ordering::SeqCst), 3);
        let state = repository.state.lock().await;
        assert_eq!(state.claim_limits, vec![1, 1, 1, 1]);
        assert_eq!(state.published.len(), 3);
        assert!(state.outstanding.is_none());
    }

    #[tokio::test]
    async fn invalid_committed_fact_is_rejected_before_any_projection_or_publication() {
        let mut invalid = message();
        invalid.event_key = "identity.*.changed".into();
        let event_id = invalid.event_id;
        let repository = Arc::new(RecordingOutboxRepository::new([invalid]));
        let publisher = Arc::new(CountingPublisher::default());
        let relay = OutboxRelay::new(repository.clone(), publisher.clone(), config(10))
            .expect("Outbox Relay");

        let report = relay.run_once().await.expect("relay turn");

        assert_eq!(report.claimed, 1);
        assert_eq!(report.published, 0);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].event_id, event_id);
        assert!(report.failures[0]
            .error
            .contains("committed integration event is invalid"));
        assert_eq!(publisher.calls.load(Ordering::SeqCst), 0);
        let state = repository.state.lock().await;
        assert_eq!(state.failed, vec![event_id]);
        assert!(state.outstanding.is_none());
    }
}
