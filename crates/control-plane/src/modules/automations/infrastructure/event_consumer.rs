use crate::modules::automations::application::IAutomationNormalizedEventHandler;
use a3s_cloud_contracts::AutomationNormalizedEventV1;
use a3s_event::{
    DeliverPolicy, EventBus, EventError, PendingEvent, ReceivedEvent, SubscribeOptions,
    SubscriptionFilter,
};
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

pub const AUTOMATION_NORMALIZED_EVENT_SUBSCRIBER_ID: &str =
    "a3s-cloud-automations-normalized-event-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationEventConsumerAction {
    Acknowledged,
    DeferredToEventProvider,
}

/// Durable provider-facing consumer for an already normalized event.
///
/// The provider owns delivery timing and redelivery. Automations only decodes
/// the strict normalized contract and delegates one event to an application
/// handler. It does not discover subscriptions, persist invocations, enqueue
/// work, or execute targets.
pub struct A3sEventAutomationNormalizedEventConsumer {
    bus: Arc<EventBus>,
    subject: String,
    source: String,
    handler: Arc<dyn IAutomationNormalizedEventHandler>,
}

impl A3sEventAutomationNormalizedEventConsumer {
    pub fn new(
        bus: Arc<EventBus>,
        subject: impl Into<String>,
        source: impl Into<String>,
        handler: Arc<dyn IAutomationNormalizedEventHandler>,
    ) -> Result<Self, String> {
        let subject = subject.into();
        let source = source.into();
        if !valid_exact_subject(&subject) {
            return Err("Automation normalized-event subject is invalid".into());
        }
        if source.is_empty() || source.contains(['\0', '\r', '\n']) {
            return Err("Automation normalized-event source is invalid".into());
        }
        Ok(Self {
            bus,
            subject,
            source,
            handler,
        })
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> a3s_event::Result<()> {
        if *shutdown.borrow() {
            return Ok(());
        }
        self.bus
            .update_subscription(SubscriptionFilter {
                subscriber_id: AUTOMATION_NORMALIZED_EVENT_SUBSCRIBER_ID.into(),
                subjects: vec![self.subject.clone()],
                durable: true,
                options: Some(SubscribeOptions {
                    // Retry timing and backpressure remain provider-owned. A
                    // successful application handoff is the ACK boundary.
                    max_deliver: None,
                    backoff_secs: Vec::new(),
                    max_ack_pending: Some(64),
                    deliver_policy: DeliverPolicy::All,
                    ack_wait_secs: Some(30),
                }),
            })
            .await?;
        let mut subscriptions = self
            .bus
            .create_subscriber(AUTOMATION_NORMALIZED_EVENT_SUBSCRIBER_ID)
            .await?;
        if subscriptions.len() != 1 {
            return Err(EventError::Config(
                "Automation normalized-event consumer requires one exact subscription".into(),
            ));
        }
        let mut subscription = subscriptions.pop().ok_or_else(|| {
            EventError::Config("Automation normalized-event subscription is missing".into())
        })?;

        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                pending = subscription.next_manual_ack() => {
                    let pending = pending?.ok_or_else(|| EventError::Consumer(
                        "Automation normalized-event subscription ended before shutdown".into()
                    ))?;
                    self.process_pending(pending).await?;
                }
            }
        }
    }

    async fn process_pending(
        &self,
        pending: PendingEvent,
    ) -> a3s_event::Result<AutomationEventConsumerAction> {
        let event_id = pending.received.event.id.clone();
        let event = match decode_normalized_event(&pending.received, &self.subject, &self.source) {
            Ok(event) => event,
            Err(error) => {
                tracing::warn!(event_id, error, "acknowledging malformed Automation event");
                pending.ack().await?;
                return Ok(AutomationEventConsumerAction::Acknowledged);
            }
        };

        match self.handler.handle(event).await {
            Ok(()) => {
                pending.ack().await?;
                Ok(AutomationEventConsumerAction::Acknowledged)
            }
            Err(_) => {
                // Do not locally sleep, nack, queue, or count retries. Dropping
                // the pending event leaves redelivery to the event provider.
                drop(pending);
                Ok(AutomationEventConsumerAction::DeferredToEventProvider)
            }
        }
    }
}

fn decode_normalized_event(
    received: &ReceivedEvent,
    expected_subject: &str,
    expected_source: &str,
) -> Result<AutomationNormalizedEventV1, String> {
    let event = &received.event;
    let event_id = Uuid::parse_str(&event.id)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| "Automation event ID is invalid".to_owned())?;
    if event.subject != expected_subject
        || event.category != "cloud"
        || event.source != expected_source
        || event.version != 1
        || received.num_delivered == 0
    {
        return Err("Automation event envelope is invalid".into());
    }
    let normalized: AutomationNormalizedEventV1 = serde_json::from_value(event.payload.clone())
        .map_err(|_| "Automation normalized event payload is invalid".to_owned())?;
    normalized
        .validate()
        .map_err(|_| "Automation normalized event payload is invalid".to_owned())?;
    if normalized.event_id != event_id || normalized.event_key != event.event_type {
        return Err("Automation event identity is inconsistent".into());
    }
    Ok(normalized)
}

fn valid_exact_subject(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.contains(['*', '>', '\0', '\r', '\n'])
        && value.split('.').count() >= 3
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::AutomationInvocationInputV1;
    use a3s_event::{Event, MemoryProvider};
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    const SUBJECT: &str = "events.cloud.plugin.package.updated";
    const SOURCE: &str = "a3s-use";

    fn timestamp(value: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(value, 0).expect("timestamp")
    }

    fn received_event() -> ReceivedEvent {
        let event_id = Uuid::from_u128(0x018f0000000070008000000000000440);
        let normalized = AutomationNormalizedEventV1::new(
            event_id,
            "plugin.package.updated",
            timestamp(1_767_229_200),
            AutomationInvocationInputV1::inline_json(json!({"package": "demo"})).expect("input"),
        )
        .expect("normalized event");
        let mut event = Event::typed(
            SUBJECT,
            "cloud",
            "plugin.package.updated",
            1,
            "plugin.package.updated",
            SOURCE,
            serde_json::to_value(normalized).expect("payload"),
        );
        event.id = event_id.to_string();
        ReceivedEvent {
            event,
            sequence: 1,
            num_delivered: 1,
            stream: "automation-test".into(),
        }
    }

    struct RecordingHandler {
        calls: AtomicUsize,
        result: Mutex<Result<(), String>>,
    }

    #[async_trait]
    impl IAutomationNormalizedEventHandler for RecordingHandler {
        async fn handle(&self, _event: AutomationNormalizedEventV1) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().expect("handler result lock").clone()
        }
    }

    fn consumer(handler: Arc<RecordingHandler>) -> A3sEventAutomationNormalizedEventConsumer {
        A3sEventAutomationNormalizedEventConsumer::new(
            Arc::new(EventBus::new(MemoryProvider::default())),
            SUBJECT,
            SOURCE,
            handler,
        )
        .expect("consumer")
    }

    fn pending(received: ReceivedEvent, acknowledgements: Arc<AtomicUsize>) -> PendingEvent {
        PendingEvent::new(
            received,
            move || {
                Box::pin(async move {
                    acknowledgements.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
            || Box::pin(async { Ok(()) }),
        )
    }

    #[test]
    fn decoder_binds_provider_identity_to_normalized_event_identity() {
        let received = received_event();
        let normalized = decode_normalized_event(&received, SUBJECT, SOURCE).expect("decoded");
        assert_eq!(normalized.event_key, "plugin.package.updated");

        let mut drifted = received;
        drifted.event.event_type = "plugin.package.deleted".into();
        assert!(decode_normalized_event(&drifted, SUBJECT, SOURCE).is_err());
    }

    #[tokio::test]
    async fn acknowledges_valid_event_after_handler_success() {
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Ok(())),
        });
        let consumer = consumer(Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received_event(), Arc::clone(&acknowledgements)))
            .await
            .expect("consumer");
        assert_eq!(action, AutomationEventConsumerAction::Acknowledged);
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn leaves_handler_failure_for_provider_redelivery() {
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Err("temporary".into())),
        });
        let consumer = consumer(Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received_event(), Arc::clone(&acknowledgements)))
            .await
            .expect("consumer");
        assert_eq!(
            action,
            AutomationEventConsumerAction::DeferredToEventProvider
        );
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn acknowledges_malformed_event_without_calling_handler() {
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Ok(())),
        });
        let consumer = consumer(Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let mut malformed = received_event();
        malformed.event.source = "untrusted-provider".into();
        let action = consumer
            .process_pending(pending(malformed, Arc::clone(&acknowledgements)))
            .await
            .expect("consumer");
        assert_eq!(action, AutomationEventConsumerAction::Acknowledged);
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_exits_before_binding_when_shutdown_is_already_requested() {
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
            result: Mutex::new(Ok(())),
        });
        let consumer = consumer(handler);
        let (_sender, shutdown) = watch::channel(true);

        tokio::time::timeout(std::time::Duration::from_secs(1), consumer.run(shutdown))
            .await
            .expect("consumer should stop promptly")
            .expect("consumer shutdown");
    }
}
