use super::*;
use crate::modules::connectors::{ConnectorExecutionEvidence, ConnectorExecutionOutcome};
use crate::modules::integration_events::OutboxMessage;
use crate::modules::notifications::{
    outbound_notification_attempt_id, IOutboundNotificationDeliveryRepository,
    IOutboundNotificationDispatcher, Notification, NotificationScope, NotificationSeverity,
    OutboundNotificationChannel, OutboundNotificationConnectorTarget,
    OutboundNotificationDeliveryAdmission, OutboundNotificationDispatchResult,
    OutboundNotificationSubscriptionDefinition, OutboundNotificationSubscriptionSpec,
    OutboundNotificationTerminalOutcome, OutboundNotificationTerminalReceipt,
    MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, InstallationId,
    NotificationId, OrganizationId, PrincipalId, ProjectId, RepositoryError, ScopeContext,
    Sha256Digest,
};
use a3s_event::{Event, MemoryProvider};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

struct RecordingDeliveryRepository {
    expected: OutboundNotificationDelivery,
    authorized: AtomicBool,
    admission_failure: AtomicBool,
    settlement_failure: AtomicBool,
    admission_calls: AtomicUsize,
    settlement_calls: AtomicUsize,
    receipt: Mutex<Option<OutboundNotificationTerminalReceipt>>,
}

impl RecordingDeliveryRepository {
    fn new(expected: OutboundNotificationDelivery) -> Self {
        Self {
            expected,
            authorized: AtomicBool::new(true),
            admission_failure: AtomicBool::new(false),
            settlement_failure: AtomicBool::new(false),
            admission_calls: AtomicUsize::new(0),
            settlement_calls: AtomicUsize::new(0),
            receipt: Mutex::new(None),
        }
    }

    fn receipt(&self) -> Option<OutboundNotificationTerminalReceipt> {
        self.receipt.lock().expect("delivery receipt lock").clone()
    }
}

#[async_trait]
impl IOutboundNotificationDeliveryRepository for RecordingDeliveryRepository {
    async fn admit_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
    ) -> Result<Option<OutboundNotificationDeliveryAdmission>, RepositoryError> {
        self.admission_calls.fetch_add(1, Ordering::SeqCst);
        if self.admission_failure.load(Ordering::SeqCst) {
            return Err(RepositoryError::Storage(
                "simulated delivery admission failure".into(),
            ));
        }
        if !self.authorized.load(Ordering::SeqCst) || delivery != &self.expected {
            return Ok(None);
        }
        Ok(Some(match self.receipt() {
            Some(receipt) => OutboundNotificationDeliveryAdmission::Terminal(receipt),
            None => OutboundNotificationDeliveryAdmission::Pending,
        }))
    }

    async fn settle_delivery(
        &self,
        delivery: &OutboundNotificationDelivery,
        receipt: OutboundNotificationTerminalReceipt,
    ) -> Result<bool, RepositoryError> {
        self.settlement_calls.fetch_add(1, Ordering::SeqCst);
        if self.settlement_failure.load(Ordering::SeqCst) {
            return Err(RepositoryError::Storage(
                "simulated delivery settlement failure".into(),
            ));
        }
        receipt
            .validate_against(delivery)
            .map_err(RepositoryError::Storage)?;
        if delivery != &self.expected {
            return Err(RepositoryError::Conflict(
                "delivery changed before settlement".into(),
            ));
        }
        let mut stored = self.receipt.lock().expect("delivery receipt lock");
        match stored.as_ref() {
            Some(existing) if existing == &receipt => Ok(false),
            Some(_) => Err(RepositoryError::Conflict(
                "another terminal receipt already exists".into(),
            )),
            None => {
                *stored = Some(receipt);
                Ok(true)
            }
        }
    }
}

fn delivery() -> OutboundNotificationDelivery {
    delivery_with_budget(None)
}

fn delivery_with_budget(maximum_provider_attempts: Option<u64>) -> OutboundNotificationDelivery {
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
    let target = OutboundNotificationConnectorTarget::new(
        ProjectId::new(),
        EnvironmentId::new(),
        ConnectorProfileId::new(),
        ConnectorRevisionId::new(),
    )
    .expect("target");
    match maximum_provider_attempts {
        Some(maximum_provider_attempts) => {
            OutboundNotificationSubscriptionDefinition::from_spec_with_provider_attempt_budget(
                OutboundNotificationSubscriptionSpec {
                    channel: OutboundNotificationChannel::SlackCompatible,
                    minimum_severity: NotificationSeverity::Warning,
                    target: target.into(),
                },
                maximum_provider_attempts,
            )
            .expect("definition")
            .delivery_for(&notification)
            .expect("version two delivery")
        }
        None => OutboundNotificationDelivery::from_notification(
            &notification,
            OutboundNotificationChannel::SlackCompatible,
            target,
        )
        .expect("delivery"),
    }
}

fn event(delivery: &OutboundNotificationDelivery, delivery_count: u64) -> ReceivedEvent {
    let fact = delivery.requested_event().expect("delivery fact");
    let scope = ScopeContext::organization(InstallationId::new(), delivery.organization_id())
        .expect("committed scope");
    let message = OutboxMessage {
        event_id: fact.event_id,
        event_key: fact.event_key.clone(),
        schema_version: fact.schema_version,
        scope,
        aggregate_id: fact.aggregate_id,
        aggregate_version: fact.aggregate_version,
        occurred_at: fact.occurred_at,
        correlation_id: fact.correlation_id,
        causation_id: fact.causation_id,
        payload: fact.payload.clone(),
        delivery_attempts: 1,
    };
    let payload = serde_json::to_value(
        PublishedOutboxEnvelope::from_message(&message).expect("published envelope"),
    )
    .expect("published envelope JSON");
    let mut event = Event::typed(
        SUBJECT,
        "cloud",
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        fact.schema_version,
        OUTBOUND_NOTIFICATION_EVENT_KEY,
        "a3s-cloud",
        payload,
    );
    event.id = fact.event_id.to_string();
    ReceivedEvent {
        event,
        sequence: 7,
        num_delivered: delivery_count,
        stream: "test".into(),
    }
}

#[test]
fn version_two_event_requires_its_matching_envelope_version() {
    let delivery = delivery_with_budget(Some(2));
    let received = event(&delivery, 1);
    assert_eq!(received.event.version, 2);
    assert_eq!(
        decode_delivery_event(&received, SUBJECT),
        Ok(delivery.clone())
    );

    let mut downgraded = received;
    downgraded.event.version = 1;
    assert!(decode_delivery_event(&downgraded, SUBJECT).is_err());
}

fn evidence(
    delivery: &OutboundNotificationDelivery,
    outcome: ConnectorExecutionOutcome,
    generation: u64,
) -> ConnectorExecutionEvidence {
    let now = canonical_timestamp(Utc::now());
    let target = delivery.connector_target().expect("Connector target");
    ConnectorExecutionEvidence::restore(
        delivery.organization_id(),
        target.project_id,
        target.environment_id,
        target.profile_id,
        target.revision_id,
        outbound_notification_attempt_id(delivery.id(), generation).expect("attempt ID"),
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
    pending_with_ack_failure(received, acknowledgements, negative_acknowledgements, false)
}

fn pending_with_ack_failure(
    received: ReceivedEvent,
    acknowledgements: Arc<AtomicUsize>,
    negative_acknowledgements: Arc<AtomicUsize>,
    fail_acknowledgement: bool,
) -> PendingEvent {
    PendingEvent::new(
        received,
        move || {
            Box::pin(async move {
                acknowledgements.fetch_add(1, Ordering::SeqCst);
                if fail_acknowledgement {
                    Err(EventError::Consumer(
                        "simulated acknowledgement loss".into(),
                    ))
                } else {
                    Ok(())
                }
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

fn consumer(
    delivery: &OutboundNotificationDelivery,
    dispatcher: Arc<RecordingDispatcher>,
) -> (
    A3sEventOutboundNotificationConsumer,
    Arc<RecordingDeliveryRepository>,
) {
    let deliveries = Arc::new(RecordingDeliveryRepository::new(delivery.clone()));
    let delivery_repository: Arc<dyn IOutboundNotificationDeliveryRepository> = deliveries.clone();
    let consumer = A3sEventOutboundNotificationConsumer::new(
        Arc::new(EventBus::new(MemoryProvider::default())),
        SUBJECT,
        delivery_repository,
        dispatcher,
    )
    .expect("consumer");
    (consumer, deliveries)
}

#[tokio::test]
async fn terminal_evidence_is_acknowledged_only_after_fenced_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Delivered {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Accepted, 1),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
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
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        deliveries.receipt().expect("terminal receipt").outcome(),
        OutboundNotificationTerminalOutcome::Delivered
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn atomically_persisted_terminal_result_is_validated_and_acked_without_second_settlement() {
    let delivery = delivery();
    let receipt = OutboundNotificationTerminalReceipt::delivered(
        &delivery,
        1,
        &evidence(&delivery, ConnectorExecutionOutcome::Accepted, 1),
    )
    .expect("terminal receipt");
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::TerminalPersisted { receipt },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("ack atomic terminal result");

    assert_eq!(action, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn exhausted_attempt_budget_is_receipted_before_acknowledgement_and_survives_ack_loss() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Exhausted {
            generation: MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
            evidence: evidence(
                &delivery,
                ConnectorExecutionOutcome::Retryable,
                MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS,
            ),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    assert!(consumer
        .process_pending(pending_with_ack_failure(
            event(
                &delivery,
                MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS + 1,
            ),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
            true,
        ))
        .await
        .is_err());

    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        deliveries.receipt().expect("terminal receipt").outcome(),
        OutboundNotificationTerminalOutcome::Exhausted
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);

    let replay = consumer
        .process_pending(pending(
            event(
                &delivery,
                MAXIMUM_OUTBOUND_NOTIFICATION_PROVIDER_ATTEMPTS + 2,
            ),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("acknowledge exhausted receipt replay");
    assert_eq!(replay, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 2);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn retryable_evidence_is_left_to_provider_ack_wait_without_local_nak() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Retryable {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Retryable, 1),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, dispatcher);
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
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn smtp_retryable_result_is_left_to_provider_ack_wait_without_local_settlement() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::SmtpRetryable {
            generation: 1,
            attempt_id: Uuid::now_v7(),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, dispatcher);
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer SMTP retryable event");

    assert_eq!(
        action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn provider_retry_after_deferral_is_left_to_event_ack_wait() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Deferred {
            generation: 1,
            attempt_id: outbound_notification_attempt_id(delivery.id(), 1).expect("attempt ID"),
            retry_not_before: canonical_timestamp(Utc::now()) + chrono::Duration::seconds(3_600),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 2),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer provider Retry-After");

    assert_eq!(
        action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 0);
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
            attempt_id: outbound_notification_attempt_id(delivery.id(), 1).expect("attempt ID"),
            dispatch_started_at: now,
            outcome_deadline_at: now + chrono::Duration::seconds(30),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
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
    assert_eq!(
        deliveries.receipt().expect("terminal receipt").outcome(),
        OutboundNotificationTerminalOutcome::Indeterminate
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn malformed_facts_are_poison_acknowledged_without_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "must not dispatch".into(),
    ))));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
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
    assert_eq!(deliveries.admission_calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unpersisted_delivery_authorization_is_poison_acknowledged_without_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "must not dispatch".into(),
    ))));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    deliveries.authorized.store(false, Ordering::SeqCst);
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    let action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("acknowledge unauthorized fact");

    assert_eq!(action, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn admission_or_receipt_failure_defers_without_acknowledgement() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Delivered {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Accepted, 1),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    deliveries.admission_failure.store(true, Ordering::SeqCst);
    let admission_action = consumer
        .process_pending(pending(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer admission failure");
    assert_eq!(
        admission_action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 0);

    deliveries.admission_failure.store(false, Ordering::SeqCst);
    deliveries.settlement_failure.store(true, Ordering::SeqCst);
    let settlement_action = consumer
        .process_pending(pending(
            event(&delivery, 2),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("defer settlement failure");
    assert_eq!(
        settlement_action,
        OutboundNotificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert!(deliveries.receipt().is_none());
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn acknowledgement_loss_replays_terminal_receipt_without_provider_dispatch() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Ok(
        OutboundNotificationDispatchResult::Delivered {
            generation: 1,
            evidence: evidence(&delivery, ConnectorExecutionOutcome::Accepted, 1),
        },
    )));
    let (consumer, deliveries) = consumer(&delivery, Arc::clone(&dispatcher));
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));

    assert!(consumer
        .process_pending(pending_with_ack_failure(
            event(&delivery, 1),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
            true,
        ))
        .await
        .is_err());
    assert!(deliveries.receipt().is_some());
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 1);

    let replay = consumer
        .process_pending(pending(
            event(&delivery, 2),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("acknowledge terminal replay");
    assert_eq!(replay, OutboundNotificationConsumerAction::Acknowledged);
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 2);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn infrastructure_failure_defers_to_a3s_event_without_cloud_retry_loop() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "database unavailable".into(),
    ))));
    let (consumer, deliveries) = consumer(&delivery, dispatcher);
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
    assert_eq!(deliveries.settlement_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn consumer_requires_one_exact_non_wildcard_subject() {
    let delivery = delivery();
    let dispatcher = Arc::new(RecordingDispatcher::new(Err(ApplicationError::Internal(
        "unused".into(),
    ))));
    let deliveries: Arc<dyn IOutboundNotificationDeliveryRepository> =
        Arc::new(RecordingDeliveryRepository::new(delivery));
    let bus = Arc::new(EventBus::new(MemoryProvider::default()));
    assert!(A3sEventOutboundNotificationConsumer::new(
        Arc::clone(&bus),
        "events.cloud.notification.>",
        deliveries.clone(),
        dispatcher.clone(),
    )
    .is_err());
    assert!(
        A3sEventOutboundNotificationConsumer::new(bus, SUBJECT, deliveries, dispatcher).is_ok()
    );
}
