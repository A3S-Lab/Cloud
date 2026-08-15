use super::*;
use crate::modules::connectors::{ConnectorExecutionEvidence, ConnectorExecutionOutcome};
use crate::modules::notifications::{
    IOutboundNotificationDispatcher, Notification, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationDispatchResult,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, NotificationId,
    OrganizationId, PrincipalId, ProjectId, Sha256Digest,
};
use a3s_event::{Event, MemoryProvider};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const SUBJECT: &str = "events.cloud.notification.delivery.requested";

struct RecordingDispatcher {
    calls: AtomicUsize,
    result: Mutex<ApplicationResult<OutboundNotificationDispatchResult>>,
}

impl RecordingDispatcher {
    fn new(result: ApplicationResult<OutboundNotificationDispatchResult>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            result: Mutex::new(result),
        }
    }
}

#[async_trait]
impl IOutboundNotificationDispatcher for RecordingDispatcher {
    async fn dispatch(
        &self,
        _delivery: &OutboundNotificationDelivery,
        _delivery_count: u64,
    ) -> ApplicationResult<OutboundNotificationDispatchResult> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.lock().expect("dispatch result lock").clone()
    }
}

fn delivery() -> OutboundNotificationDelivery {
    let now = canonical_timestamp(Utc::now());
    let notification = Notification::project(
        OrganizationId::new(),
        PrincipalId::new(),
        Uuid::now_v7(),
        "identity.membership.role-changed".into(),
        1,
        Uuid::now_v7(),
        2,
        Uuid::now_v7(),
        NotificationSeverity::Warning,
        "Organization role changed".into(),
        "Your organization role is now member.".into(),
        NotificationScope::Organization,
        now,
        now,
    )
    .expect("notification");
    OutboundNotificationDelivery::from_notification(
        &notification,
        OutboundNotificationChannel::SlackCompatible,
        OutboundNotificationConnectorTarget::new(
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
        )
        .expect("target"),
    )
    .expect("delivery")
}

fn event(delivery: &OutboundNotificationDelivery, delivery_count: u64) -> ReceivedEvent {
    let fact = delivery.requested_event().expect("delivery fact");
    let mut event = Event::typed(
        SUBJECT,
        "cloud",
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        1,
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        "a3s-cloud",
        serde_json::json!({
            "organizationId": fact.organization_id,
            "aggregateId": fact.aggregate_id,
            "aggregateVersion": fact.aggregate_version,
            "occurredAt": fact.occurred_at,
            "correlationId": fact.correlation_id,
            "causationId": fact.causation_id,
            "data": fact.payload,
        }),
    );
    event.id = fact.event_id.to_string();
    ReceivedEvent {
        event,
        sequence: 7,
        num_delivered: delivery_count,
        stream: "test".into(),
    }
}

fn evidence(
    delivery: &OutboundNotificationDelivery,
    outcome: ConnectorExecutionOutcome,
) -> ConnectorExecutionEvidence {
    let now = canonical_timestamp(Utc::now());
    let target = delivery.target();
    ConnectorExecutionEvidence::restore(
        delivery.organization_id(),
        target.project_id,
        target.environment_id,
        target.profile_id,
        target.revision_id,
        Uuid::now_v7(),
        Sha256Digest::from_bytes(b"request"),
        7,
        outcome,
        (outcome == ConnectorExecutionOutcome::Accepted).then_some(204),
        (outcome == ConnectorExecutionOutcome::Accepted)
            .then(|| Sha256Digest::from_bytes(b"response")),
        (outcome == ConnectorExecutionOutcome::Accepted).then_some(0),
        None,
        now,
        now,
    )
    .expect("evidence")
}

fn pending(
    received: ReceivedEvent,
    acknowledgements: Arc<AtomicUsize>,
    negative_acknowledgements: Arc<AtomicUsize>,
) -> PendingEvent {
    PendingEvent::new(
        received,
        move || {
            Box::pin(async move {
                acknowledgements.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        },
        move || {
            Box::pin(async move {
                negative_acknowledgements.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        },
    )
}

fn consumer(dispatcher: Arc<RecordingDispatcher>) -> A3sEventOutboundNotificationConsumer {
    A3sEventOutboundNotificationConsumer::new(
        Arc::new(EventBus::new(MemoryProvider::default())),
        SUBJECT,
        dispatcher,
    )
    .expect("consumer")
}

#[tokio::test]
async fn terminal_evidence_is_acknowledged_only_after_fenced_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Delivered {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Accepted),
        },
    )));
    let consumer = consumer(Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("process terminal event");

    assert_eq!(action, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn retryable_evidence_is_left_to_provider_ack_wait_without_local_nak() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Retryable {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Retryable),
        },
    )));
    let consumer = consumer(dispatcher);
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer retryable event");

    assert_eq!(
        action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn indeterminate_attempt_is_acknowledged_without_blind_provider_retry() {
    let delivery = delivery();
    let now = canonical_timestamp(Utc::now());
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Indeterminate {
            generation: 1,
            attempt_id: Uuid::now_v7(),
            dispatch_started_at: now,
            outcome_deadline_at: now + chrono::Duration::seconds(30),
        },
    )));
    let consumer = consumer(Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 2),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("acknowledge indeterminate attempt");

    assert_eq!(action, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn malformed_facts_are_poison_acknowledged_without_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "must not dispatch".into(),
    ))));
    let consumer = consumer(Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));
    let mut malformed = event(&delivery, 1);
    malformed.event.payload["aggregateId"] = serde_json::json!(NotificationId::new());

    let action = consumer
        .process_pending(pending(
            malformed,
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("ack poison event");

    assert_eq!(action, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn infrastructure_failure_defers_to_a3s_event_without_cloud_retry_loop() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "database unavailable".into(),
    ))));
    let consumer = consumer(dispatcher);
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 4),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer infrastructure failure");

    assert_eq!(
        action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[test]
fn consumer_requires_one_exact_non_wildcard_subject() {
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "unused".into(),
    ))));
    let bus = Arc::new(EventBus::new(MemoryProvider::default()));
    assert!(A3sEventOutboundNotificationConsumer::new(
        Arc::clone(&bus),
        "events.cloud.notification.>",
        dispatcher.clone(),
    )
    .is_err());
    assert!(A3sEventOutboundNotificationConsumer::new(bus, SUBJECT, dispatcher).is_ok());
}
