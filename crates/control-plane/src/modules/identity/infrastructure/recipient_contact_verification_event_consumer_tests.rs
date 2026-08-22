use super::*;
use crate::modules::identity::domain::entities::{
    RecipientContact, RecipientContactVerificationDeliveryStatus,
};
use crate::modules::identity::domain::value_objects::{
    RecipientContactSigningKeyId, RecipientEmailAddress,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, OrganizationId, PrincipalId, RecipientContactId,
    RecipientContactVerificationId,
};
use a3s_event::{Event, MemoryProvider};
use async_trait::async_trait;
use chrono::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const SUBJECT: &str = "events.cloud.identity.recipient-contact.verification-requested";

struct RecordingDispatcher {
    calls: AtomicUsize,
    result: Mutex<Result<RecipientContactVerificationDispatchResult, String>>,
}

#[async_trait]
impl IRecipientContactVerificationDispatcher for RecordingDispatcher {
    async fn dispatch(
        &self,
        _fact: &RecipientContactVerificationDeliveryFact,
    ) -> Result<RecipientContactVerificationDispatchResult, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result.lock().expect("dispatcher result lock").clone()
    }
}

fn received_event() -> ReceivedEvent {
    let now = canonical_timestamp(Utc::now());
    let organization_id = OrganizationId::new();
    let contact = RecipientContact::create(
        RecipientContactId::new(),
        PrincipalId::new(),
        RecipientEmailAddress::parse("private@example.test").expect("address"),
        now,
    )
    .expect("contact");
    let verification = RecipientContactVerification::create(
        RecipientContactVerificationId::new(),
        contact.id,
        contact.principal_id,
        contact.address.digest(),
        contact.aggregate_version,
        RecipientContactSigningKeyId::parse("contact-v1").expect("key ID"),
        now,
        now + Duration::minutes(10),
    )
    .expect("verification");
    let outbox = RecipientContactChanged::verification_requested(
        organization_id,
        &contact.record(),
        &verification,
        Uuid::now_v7(),
    )
    .expect("Outbox fact");
    let mut event = Event::typed(
        SUBJECT,
        "cloud",
        RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY,
        outbox.schema_version,
        RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY,
        "a3s-cloud",
        serde_json::json!({
            "organizationId": outbox.organization_id,
            "aggregateId": outbox.aggregate_id,
            "aggregateVersion": outbox.aggregate_version,
            "occurredAt": outbox.occurred_at,
            "correlationId": outbox.correlation_id,
            "causationId": outbox.causation_id,
            "data": outbox.payload,
        }),
    );
    event.id = outbox.event_id.to_string();
    ReceivedEvent {
        event,
        sequence: 1,
        num_delivered: 1,
        stream: "test".into(),
    }
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

fn consumer(dispatcher: Arc<RecordingDispatcher>) -> A3sEventRecipientContactVerificationConsumer {
    A3sEventRecipientContactVerificationConsumer::new(
        Arc::new(EventBus::new(MemoryProvider::default())),
        SUBJECT,
        dispatcher,
    )
    .expect("consumer")
}

#[test]
fn exact_fact_decodes_and_envelope_or_payload_drift_fails_closed() {
    let received = received_event();
    let fact = decode_verification_requested_event(&received, SUBJECT).expect("exact fact");
    assert_eq!(fact.id().as_uuid().to_string(), received.event.id);
    assert!(!serde_json::to_string(&received.event.payload)
        .expect("event JSON")
        .contains("private@example.test"));

    let mut drifted = received;
    drifted.event.payload["aggregateVersion"] = serde_json::json!(2);
    assert!(decode_verification_requested_event(&drifted, SUBJECT).is_err());

    let mut extended = received_event();
    extended.event.payload["data"]["unexpected"] = serde_json::json!(true);
    assert!(decode_verification_requested_event(&extended, SUBJECT).is_err());
}

#[tokio::test]
async fn terminal_delivery_is_acknowledged_and_deferred_delivery_uses_ack_wait() {
    let terminal_dispatcher = Arc::new(RecordingDispatcher {
        calls: AtomicUsize::new(0),
        result: Mutex::new(Ok(RecipientContactVerificationDispatchResult::Terminal(
            RecipientContactVerificationDeliveryStatus::Delivered,
        ))),
    });
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));
    let action = consumer(Arc::clone(&terminal_dispatcher))
        .process_pending(pending(
            received_event(),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("terminal event");
    assert_eq!(
        action,
        RecipientContactVerificationConsumerAction::Acknowledged
    );
    assert_eq!(terminal_dispatcher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);

    let deferred_dispatcher = Arc::new(RecordingDispatcher {
        calls: AtomicUsize::new(0),
        result: Mutex::new(Ok(RecipientContactVerificationDispatchResult::Deferred)),
    });
    let action = consumer(deferred_dispatcher)
        .process_pending(pending(
            received_event(),
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("deferred event");
    assert_eq!(
        action,
        RecipientContactVerificationConsumerAction::DeferredToEventProvider
    );
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn malformed_fact_is_poison_acknowledged_without_dispatch() {
    let dispatcher = Arc::new(RecordingDispatcher {
        calls: AtomicUsize::new(0),
        result: Mutex::new(Err("must not dispatch".into())),
    });
    let mut malformed = received_event();
    malformed.event.payload["data"]["state"] = serde_json::json!("verified");
    let acknowledgements = Arc::new(AtomicUsize::new(0));
    let negative_acknowledgements = Arc::new(AtomicUsize::new(0));
    let action = consumer(Arc::clone(&dispatcher))
        .process_pending(pending(
            malformed,
            Arc::clone(&acknowledgements),
            Arc::clone(&negative_acknowledgements),
        ))
        .await
        .expect("malformed event");
    assert_eq!(
        action,
        RecipientContactVerificationConsumerAction::Acknowledged
    );
    assert_eq!(dispatcher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
    assert_eq!(negative_acknowledgements.load(Ordering::SeqCst), 0);
}
