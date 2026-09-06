use crate::modules::automations::application::IAutomationInvocationHandler;
use crate::modules::automations::domain::{
    AutomationInvocationRecord, IAutomationInvocationReader,
};
use crate::modules::integration_events::PublishedOutboxEnvelope;
use a3s_cloud_contracts::{AutomationOutboxEventKindV1, AutomationOutboxMessageV1};
use a3s_event::{
    DeliverPolicy, EventBus, EventError, PendingEvent, ReceivedEvent, SubscribeOptions,
    SubscriptionFilter,
};
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

pub const AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY: &str = "automation.invocation.admitted";
pub const AUTOMATION_INVOCATION_SUBSCRIBER_ID: &str = "a3s-cloud-automations-invocation-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationInvocationConsumerAction {
    Acknowledged,
    DeferredToEventProvider,
}

/// Durable target-owner handoff for one committed Automation invocation.
///
/// The published event contains only the exact invocation digest and identity.
/// This consumer reads the immutable envelope from the admission repository,
/// verifies the digest and scope, and then delegates to an injected target
/// owner. It never executes a target or creates a retry queue locally.
pub struct A3sEventAutomationInvocationConsumer {
    bus: Arc<EventBus>,
    subject: String,
    source: String,
    invocations: Arc<dyn IAutomationInvocationReader>,
    handler: Arc<dyn IAutomationInvocationHandler>,
}

impl A3sEventAutomationInvocationConsumer {
    pub fn new(
        bus: Arc<EventBus>,
        subject: impl Into<String>,
        source: impl Into<String>,
        invocations: Arc<dyn IAutomationInvocationReader>,
        handler: Arc<dyn IAutomationInvocationHandler>,
    ) -> Result<Self, String> {
        let subject = subject.into();
        let source = source.into();
        if !valid_exact_subject(&subject) {
            return Err("Automation invocation subject is invalid".into());
        }
        if source.is_empty() || source.contains(['\0', '\r', '\n']) {
            return Err("Automation invocation source is invalid".into());
        }
        Ok(Self {
            bus,
            subject,
            source,
            invocations,
            handler,
        })
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> a3s_event::Result<()> {
        self.bus
            .update_subscription(SubscriptionFilter {
                subscriber_id: AUTOMATION_INVOCATION_SUBSCRIBER_ID.into(),
                subjects: vec![self.subject.clone()],
                durable: true,
                options: Some(SubscribeOptions {
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
            .create_subscriber(AUTOMATION_INVOCATION_SUBSCRIBER_ID)
            .await?;
        if subscriptions.len() != 1 {
            return Err(EventError::Config(
                "Automation invocation consumer requires one exact subscription".into(),
            ));
        }
        let mut subscription = subscriptions.pop().ok_or_else(|| {
            EventError::Config("Automation invocation subscription is missing".into())
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
                        "Automation invocation subscription ended before shutdown".into()
                    ))?;
                    self.process_pending(pending).await?;
                }
            }
        }
    }

    async fn process_pending(
        &self,
        pending: PendingEvent,
    ) -> a3s_event::Result<AutomationInvocationConsumerAction> {
        let event_id = pending.received.event.id.clone();
        let (organization_id, message) =
            match decode_message(&pending.received, &self.subject, &self.source) {
                Ok(message) => message,
                Err(error) => {
                    tracing::warn!(
                        event_id,
                        error,
                        "acknowledging malformed Automation invocation event"
                    );
                    pending.ack().await?;
                    return Ok(AutomationInvocationConsumerAction::Acknowledged);
                }
            };
        let Some(invocation_id) = message.invocation_id else {
            tracing::error!("acknowledging Automation invocation event without an invocation ID");
            pending.ack().await?;
            return Ok(AutomationInvocationConsumerAction::Acknowledged);
        };
        let Some(invocation) = (match self.invocations.find(organization_id, invocation_id).await {
            Ok(invocation) => invocation,
            Err(error) => {
                tracing::warn!(%error, %invocation_id, "deferring Automation invocation lookup to event provider");
                drop(pending);
                return Ok(AutomationInvocationConsumerAction::DeferredToEventProvider);
            }
        }) else {
            tracing::warn!(%invocation_id, "deferring Automation invocation until durable record is visible");
            drop(pending);
            return Ok(AutomationInvocationConsumerAction::DeferredToEventProvider);
        };

        if !matches_invocation_message(&invocation, &message) {
            tracing::error!(%invocation_id, "acknowledging Automation invocation digest or scope drift");
            pending.ack().await?;
            return Ok(AutomationInvocationConsumerAction::Acknowledged);
        }

        match self.handler.handle(invocation).await {
            Ok(()) => {
                pending.ack().await?;
                Ok(AutomationInvocationConsumerAction::Acknowledged)
            }
            Err(error) => {
                tracing::warn!(%error, %invocation_id, "deferring Automation target handoff to event provider");
                drop(pending);
                Ok(AutomationInvocationConsumerAction::DeferredToEventProvider)
            }
        }
    }
}

fn decode_message(
    received: &ReceivedEvent,
    expected_subject: &str,
    expected_source: &str,
) -> Result<(Uuid, AutomationOutboxMessageV1), String> {
    let event = &received.event;
    let event_id = Uuid::parse_str(&event.id)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| "Automation invocation event ID is invalid".to_owned())?;
    if event.subject != expected_subject
        || event.category != "cloud"
        || event.source != expected_source
        || event.version != 1
        || event.event_type != AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY
        || received.num_delivered == 0
    {
        return Err("Automation invocation event envelope is invalid".into());
    }
    let published: PublishedOutboxEnvelope = serde_json::from_value(event.payload.clone())
        .map_err(|_| "Automation published Outbox envelope is invalid".to_owned())?;
    published
        .validate()
        .map_err(|_| "Automation published Outbox envelope is invalid".to_owned())?;
    let organization_id = published.require_tenant_organization_id()?;
    let message: AutomationOutboxMessageV1 = serde_json::from_value(published.data().clone())
        .map_err(|_| "Automation invocation Outbox message is invalid".to_owned())?;
    message
        .validate()
        .map_err(|_| "Automation invocation Outbox message is invalid".to_owned())?;
    if message.kind != AutomationOutboxEventKindV1::InvocationAdmitted
        || message.message_id != event_id
        || message.event_key() != event.event_type
        || message.organization_id != organization_id
        || published.aggregate_id() != message.automation_id
        || published.aggregate_version() != 1
        || published.occurred_at() != message.occurred_at
        || published.correlation_id() != message.correlation_id
        || published.causation_id() != message.causation_id
    {
        return Err("Automation invocation Outbox identity is inconsistent".into());
    }
    let scope = published.scope();
    if scope.project_id().map(|value| value.as_uuid()) != Some(message.project_id)
        || scope.environment_id().map(|value| value.as_uuid()) != Some(message.environment_id)
    {
        return Err("Automation invocation Outbox scope is inconsistent".into());
    }
    Ok((organization_id, message))
}

fn matches_invocation_message(
    invocation: &AutomationInvocationRecord,
    message: &AutomationOutboxMessageV1,
) -> bool {
    invocation.digest == message.payload_digest
        && invocation.envelope.organization_id == message.organization_id
        && invocation.envelope.project_id == message.project_id
        && invocation.envelope.environment_id == message.environment_id
        && invocation.envelope.automation_id == message.automation_id
        && invocation.envelope.automation_revision_id == message.revision_id
        && message.invocation_id == Some(invocation.envelope.invocation_id)
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
    use crate::modules::automations::domain::{
        AutomationScheduleInvocationFactory, AutomationScheduleInvocationRequest,
        IAutomationInvocationRepository,
    };
    use crate::modules::automations::infrastructure::InMemoryAutomationInvocationRepository;
    use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationInvocationAuthorizationV1, AutomationInvocationInputV1,
        AutomationRevisionV1,
    };
    use a3s_event::{Event, MemoryProvider};
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SUBJECT: &str = "events.cloud.automation.invocation.admitted";
    const SOURCE: &str = "a3s-cloud";
    const SCHEDULE_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    struct RecordingHandler {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl IAutomationInvocationHandler for RecordingHandler {
        async fn handle(&self, _invocation: AutomationInvocationRecord) -> ApplicationResult<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FailingHandler;

    #[async_trait]
    impl IAutomationInvocationHandler for FailingHandler {
        async fn handle(&self, _invocation: AutomationInvocationRecord) -> ApplicationResult<()> {
            Err(ApplicationError::Unavailable(
                "target owner is unavailable".into(),
            ))
        }
    }

    fn revision() -> AutomationRevisionV1 {
        let definition =
            AutomationDefinitionV1::parse_acl(SCHEDULE_DEFINITION).expect("definition");
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000601),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision")
    }

    fn envelope() -> a3s_cloud_contracts::AutomationInvocationEnvelopeV1 {
        let revision = revision();
        AutomationScheduleInvocationFactory::build(AutomationScheduleInvocationRequest {
            revision: &revision,
            invocation_id: Uuid::from_u128(0x018f0000000070008000000000000602),
            scheduled_at: Utc.timestamp_opt(1_789_000_000, 0).single().expect("time"),
            requested_at: Utc.timestamp_opt(1_789_000_001, 0).single().expect("time"),
            input: AutomationInvocationInputV1::inline_json(json!({"source": "consumer-test"}))
                .expect("input"),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: revision
                    .spec()
                    .definition
                    .authorization
                    .policy_digest
                    .clone(),
                grant_snapshot_digest: format!("sha256:{}", "c".repeat(64)),
                principal_id: None,
            },
            correlation_id: Uuid::from_u128(0x018f0000000070008000000000000603),
            causation_id: None,
        })
        .expect("invocation")
    }

    fn received(message: &AutomationOutboxMessageV1) -> ReceivedEvent {
        let organization_id = message.organization_id.to_string();
        let project_id = message.project_id.to_string();
        let environment_id = message.environment_id.to_string();
        let payload = json!({
            "scope": {
                "kind": "environment",
                "installation_id": Uuid::from_u128(0x018f0000000070008000000000000604).to_string(),
                "organization_id": organization_id,
                "project_id": project_id,
                "environment_id": environment_id,
            },
            "organizationId": organization_id,
            "aggregateId": message.automation_id,
            "aggregateVersion": 1,
            "occurredAt": message.occurred_at,
            "correlationId": message.correlation_id,
            "causationId": message.causation_id,
            "data": message,
        });
        let mut event = Event::typed(
            SUBJECT,
            "cloud",
            AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY,
            1,
            AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY,
            SOURCE,
            payload,
        );
        event.id = message.message_id.to_string();
        ReceivedEvent {
            event,
            sequence: 1,
            num_delivered: 1,
            stream: "automation-invocation-test".into(),
        }
    }

    fn pending(received: ReceivedEvent, acknowledgements: Arc<AtomicUsize>) -> PendingEvent {
        PendingEvent::new(
            received,
            move || {
                let acknowledgements = Arc::clone(&acknowledgements);
                Box::pin(async move {
                    acknowledgements.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            },
            || Box::pin(async { Ok(()) }),
        )
    }

    fn consumer(
        repository: Arc<InMemoryAutomationInvocationRepository>,
        handler: Arc<RecordingHandler>,
    ) -> A3sEventAutomationInvocationConsumer {
        A3sEventAutomationInvocationConsumer::new(
            Arc::new(EventBus::new(MemoryProvider::default())),
            SUBJECT,
            SOURCE,
            repository,
            handler,
        )
        .expect("consumer")
    }

    #[test]
    fn decoder_requires_exact_published_scope_and_message_identity() {
        let invocation = envelope();
        let message = AutomationOutboxMessageV1::for_invocation(
            &invocation,
            Uuid::from_u128(0x018f0000000070008000000000000605),
            None,
            invocation.requested_at,
        )
        .expect("message");
        let received = received(&message);
        let (organization_id, decoded) =
            decode_message(&received, SUBJECT, SOURCE).expect("decode");
        assert_eq!(organization_id, invocation.organization_id);
        assert_eq!(decoded, message);

        let mut drifted = received;
        drifted.event.event_type = "automation.invocation.replayed".into();
        assert!(decode_message(&drifted, SUBJECT, SOURCE).is_err());
    }

    #[tokio::test]
    async fn consumer_reloads_exact_record_before_acknowledging_success() {
        let repository = Arc::new(InMemoryAutomationInvocationRepository::new());
        let invocation = envelope();
        repository.admit(invocation.clone()).await.expect("admit");
        let message = repository
            .outbox_messages()
            .await
            .pop()
            .expect("outbox message");
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
        });
        let consumer = consumer(Arc::clone(&repository), Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received(&message), Arc::clone(&acknowledgements)))
            .await
            .expect("process");
        assert_eq!(action, AutomationInvocationConsumerAction::Acknowledged);
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
        assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn missing_record_is_left_for_provider_redelivery() {
        let repository = Arc::new(InMemoryAutomationInvocationRepository::new());
        let invocation = envelope();
        let message = AutomationOutboxMessageV1::for_invocation(
            &invocation,
            Uuid::from_u128(0x018f0000000070008000000000000606),
            None,
            invocation.requested_at,
        )
        .expect("message");
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
        });
        let consumer = consumer(repository, Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received(&message), Arc::clone(&acknowledgements)))
            .await
            .expect("process");
        assert_eq!(
            action,
            AutomationInvocationConsumerAction::DeferredToEventProvider
        );
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn digest_drift_is_acknowledged_without_target_execution() {
        let repository = Arc::new(InMemoryAutomationInvocationRepository::new());
        let invocation = envelope();
        repository.admit(invocation.clone()).await.expect("admit");
        let mut message = repository
            .outbox_messages()
            .await
            .pop()
            .expect("outbox message");
        message.payload_digest = format!("sha256:{}", "d".repeat(64));
        let handler = Arc::new(RecordingHandler {
            calls: AtomicUsize::new(0),
        });
        let consumer = consumer(Arc::clone(&repository), Arc::clone(&handler));
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received(&message), Arc::clone(&acknowledgements)))
            .await
            .expect("process");
        assert_eq!(action, AutomationInvocationConsumerAction::Acknowledged);
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 1);
        assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn target_owner_failure_is_left_for_provider_redelivery() {
        let repository = Arc::new(InMemoryAutomationInvocationRepository::new());
        let invocation = envelope();
        repository.admit(invocation.clone()).await.expect("admit");
        let message = repository
            .outbox_messages()
            .await
            .pop()
            .expect("outbox message");
        let reader: Arc<dyn IAutomationInvocationReader> = repository;
        let consumer = A3sEventAutomationInvocationConsumer::new(
            Arc::new(EventBus::new(MemoryProvider::default())),
            SUBJECT,
            SOURCE,
            reader,
            Arc::new(FailingHandler),
        )
        .expect("consumer");
        let acknowledgements = Arc::new(AtomicUsize::new(0));
        let action = consumer
            .process_pending(pending(received(&message), Arc::clone(&acknowledgements)))
            .await
            .expect("process");
        assert_eq!(
            action,
            AutomationInvocationConsumerAction::DeferredToEventProvider
        );
        assert_eq!(acknowledgements.load(Ordering::SeqCst), 0);
    }
}
