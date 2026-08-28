use crate::modules::integration_events::PublishedOutboxEnvelope;
use crate::modules::notifications::application::{
    IOutboundNotificationDispatcher, OutboundNotificationDispatchResult,
};
use crate::modules::notifications::domain::{
    IOutboundNotificationDeliveryRepository, OutboundNotificationDelivery,
    OutboundNotificationDeliveryAdmission, OutboundNotificationTerminalReceipt,
    OUTBOUND_NOTIFICATION_EVENT_KEY,
};
use a3s_event::{
    DeliverPolicy, EventBus, EventError, PendingEvent, ReceivedEvent, SubscribeOptions,
    SubscriptionFilter,
};
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

pub const OUTBOUND_NOTIFICATION_SUBSCRIBER_ID: &str = "a3s-cloud-notification-delivery-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundNotificationConsumerAction {
    Acknowledged,
    DeferredToEventProvider,
}

/// Consumes Notification-owned delivery facts through A3S Event's durable,
/// explicit-acknowledgement path.
///
/// Retry timing and backpressure are provider-owned. On a retryable result the
/// pending event is deliberately left unacknowledged, allowing NATS AckWait to
/// drive redelivery without a Cloud-local sleep loop, queue, or retry counter.
pub struct A3sEventOutboundNotificationConsumer {
    bus: Arc<EventBus>,
    subject: String,
    deliveries: Arc<dyn IOutboundNotificationDeliveryRepository>,
    dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
}

impl A3sEventOutboundNotificationConsumer {
    pub fn new(
        bus: Arc<EventBus>,
        subject: impl Into<String>,
        deliveries: Arc<dyn IOutboundNotificationDeliveryRepository>,
        dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
    ) -> Result<Self, String> {
        let subject = subject.into();
        if !valid_exact_subject(&subject) {
            return Err("outbound notification A3S Event subject is invalid".into());
        }
        Ok(Self {
            bus,
            subject,
            deliveries,
            dispatcher,
        })
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> a3s_event::Result<()> {
        self.bus
            .update_subscription(SubscriptionFilter {
                subscriber_id: OUTBOUND_NOTIFICATION_SUBSCRIBER_ID.into(),
                subjects: vec![self.subject.clone()],
                durable: true,
                options: Some(SubscribeOptions {
                    // Provider delivery counts include infrastructure redeliveries, so
                    // keep that policy provider-owned and unbounded. Persisted terminal
                    // receipts make all post-terminal replays ACK-only.
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
            .create_subscriber(OUTBOUND_NOTIFICATION_SUBSCRIBER_ID)
            .await?;
        if subscriptions.len() != 1 {
            return Err(EventError::Config(
                "outbound notification consumer requires one exact subscription".into(),
            ));
        }
        let mut subscription = subscriptions.pop().ok_or_else(|| {
            EventError::Config("outbound notification subscription is missing".into())
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
                        "outbound notification subscription ended before shutdown".into()
                    ))?;
                    self.process_pending(pending).await?;
                }
            }
        }
    }

    async fn process_pending(
        &self,
        pending: PendingEvent,
    ) -> a3s_event::Result<OutboundNotificationConsumerAction> {
        let event_id = pending.received.event.id.clone();
        let delivery_count = pending.received.num_delivered;
        let delivery = match decode_delivery_event(&pending.received, &self.subject) {
            Ok(delivery) => delivery,
            Err(error) => {
                tracing::warn!(
                    event_id,
                    error,
                    "acknowledging malformed outbound notification fact"
                );
                pending.ack().await?;
                return Ok(OutboundNotificationConsumerAction::Acknowledged);
            }
        };

        match self.deliveries.admit_delivery(&delivery).await {
            Ok(Some(OutboundNotificationDeliveryAdmission::Pending)) => {}
            Ok(Some(OutboundNotificationDeliveryAdmission::Terminal(receipt))) => {
                if let Err(error) = receipt.validate_against(&delivery) {
                    tracing::error!(
                        event_id,
                        delivery_id = %delivery.id(),
                        error,
                        "leaving outbound notification fact unacknowledged after invalid stored receipt"
                    );
                    drop(pending);
                    return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
                }
                tracing::debug!(
                    event_id,
                    delivery_id = %delivery.id(),
                    "acknowledging outbound notification fact from its terminal receipt"
                );
                pending.ack().await?;
                return Ok(OutboundNotificationConsumerAction::Acknowledged);
            }
            Ok(None) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    "acknowledging outbound notification fact without persisted delivery authorization"
                );
                pending.ack().await?;
                return Ok(OutboundNotificationConsumerAction::Acknowledged);
            }
            Err(error) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error = %error,
                    "leaving outbound notification fact unacknowledged while admission is unavailable"
                );
                drop(pending);
                return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
            }
        }

        let receipt = match self.dispatcher.dispatch(&delivery, delivery_count).await {
            Ok(OutboundNotificationDispatchResult::TerminalPersisted { receipt }) => {
                if let Err(error) = receipt.validate_against(&delivery) {
                    tracing::error!(
                        event_id,
                        delivery_id = %delivery.id(),
                        error,
                        "leaving outbound SMTP notification fact unacknowledged after invalid atomic terminal receipt"
                    );
                    drop(pending);
                    return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
                }
                pending.ack().await?;
                return Ok(OutboundNotificationConsumerAction::Acknowledged);
            }
            Ok(OutboundNotificationDispatchResult::Delivered {
                generation,
                evidence,
            }) => OutboundNotificationTerminalReceipt::delivered(&delivery, generation, &evidence),
            Ok(OutboundNotificationDispatchResult::Rejected {
                generation,
                evidence,
            }) => OutboundNotificationTerminalReceipt::rejected(&delivery, generation, &evidence),
            Ok(OutboundNotificationDispatchResult::Exhausted {
                generation,
                evidence,
            }) => OutboundNotificationTerminalReceipt::exhausted(&delivery, generation, &evidence),
            Ok(OutboundNotificationDispatchResult::Indeterminate {
                generation,
                attempt_id,
                outcome_deadline_at,
                ..
            }) => OutboundNotificationTerminalReceipt::indeterminate(
                &delivery,
                generation,
                attempt_id,
                outcome_deadline_at,
            ),
            Ok(
                OutboundNotificationDispatchResult::Retryable { .. }
                | OutboundNotificationDispatchResult::SmtpRetryable { .. }
                | OutboundNotificationDispatchResult::Deferred { .. },
            ) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    delivery_count,
                    "leaving outbound notification fact unacknowledged for A3S Event redelivery"
                );
                drop(pending);
                return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
            }
            Err(error) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error = %error,
                    "leaving outbound notification fact unacknowledged after dispatch failure"
                );
                drop(pending);
                return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
            }
        };
        let receipt = match receipt {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::error!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error,
                    "leaving outbound notification fact unacknowledged after invalid terminal evidence"
                );
                drop(pending);
                return Ok(OutboundNotificationConsumerAction::DeferredToEventProvider);
            }
        };

        match self.deliveries.settle_delivery(&delivery, receipt).await {
            Ok(_) => {
                pending.ack().await?;
                Ok(OutboundNotificationConsumerAction::Acknowledged)
            }
            Err(error) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error = %error,
                    "leaving outbound notification fact unacknowledged until terminal receipt is durable"
                );
                drop(pending);
                Ok(OutboundNotificationConsumerAction::DeferredToEventProvider)
            }
        }
    }
}

fn decode_delivery_event(
    received: &ReceivedEvent,
    expected_subject: &str,
) -> Result<OutboundNotificationDelivery, String> {
    let event = &received.event;
    let event_id = Uuid::parse_str(&event.id)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| "outbound notification event ID is invalid".to_owned())?;
    if event.subject != expected_subject
        || event.category != "cloud"
        || event.event_type != OUTBOUND_NOTIFICATION_EVENT_KEY
        || event.source != "a3s-cloud"
        || received.num_delivered == 0
    {
        return Err("outbound notification event envelope is invalid".into());
    }
    let envelope: PublishedOutboxEnvelope = serde_json::from_value(event.payload.clone())
        .map_err(|_| "outbound notification Outbox envelope is invalid".to_owned())?;
    envelope
        .validate()
        .map_err(|_| "outbound notification Outbox envelope is invalid".to_owned())?;
    let organization_id = envelope.require_tenant_organization_id()?;
    let delivery = OutboundNotificationDelivery::from_payload(envelope.data())?;
    if event.version != delivery.schema_version()
        || organization_id != delivery.organization_id().as_uuid()
        || envelope.aggregate_id() != delivery.id()
        || envelope.aggregate_version() != 1
        || envelope.occurred_at() != delivery.occurred_at()
        || envelope.correlation_id() != delivery.correlation_id()
        || envelope.causation_id() != Some(delivery.source_event_id())
        || event_id != delivery.requested_event_id()
    {
        return Err("outbound notification fact identity is inconsistent".into());
    }
    Ok(delivery)
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
#[path = "outbound_event_consumer_tests.rs"]
mod tests;
