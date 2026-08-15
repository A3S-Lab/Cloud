use crate::modules::notifications::application::{
    IOutboundNotificationDispatcher, OutboundNotificationDispatchResult,
};
use crate::modules::notifications::domain::{
    OutboundNotificationDelivery, OUTBOUND_NOTIFICATION_EVENT_KEY,
};
use crate::modules::shared_kernel::application::ApplicationError;
use a3s_event::{
    DeliverPolicy, EventBus, EventError, PendingEvent, ReceivedEvent, SubscribeOptions,
    SubscriptionFilter,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
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
    dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
}

impl A3sEventOutboundNotificationConsumer {
    pub fn new(
        bus: Arc<EventBus>,
        subject: impl Into<String>,
        dispatcher: Arc<dyn IOutboundNotificationDispatcher>,
    ) -> Result<Self, String> {
        let subject = subject.into();
        if !valid_exact_subject(&subject) {
            return Err("outbound notification A3S Event subject is invalid".into());
        }
        Ok(Self {
            bus,
            subject,
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
                    // Logical terminal receipts will close a later bounded-delivery
                    // policy. Until then, fail closed with unlimited provider-owned
                    // redelivery instead of silently dropping a committed fact.
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

        match self.dispatcher.dispatch(&delivery, delivery_count).await {
            Ok(result) if result.should_acknowledge() => {
                pending.ack().await?;
                Ok(OutboundNotificationConsumerAction::Acknowledged)
            }
            Ok(OutboundNotificationDispatchResult::Indeterminate {
                generation,
                attempt_id,
                ..
            }) => {
                tracing::error!(
                    event_id,
                    delivery_id = %delivery.id(),
                    generation,
                    attempt_id = %attempt_id,
                    "acknowledging indeterminate fenced Connector attempt without retrying the provider"
                );
                pending.ack().await?;
                Ok(OutboundNotificationConsumerAction::Acknowledged)
            }
            Ok(_) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    delivery_count,
                    "leaving outbound notification fact unacknowledged for A3S Event redelivery"
                );
                drop(pending);
                Ok(OutboundNotificationConsumerAction::DeferredToEventProvider)
            }
            Err(error) if terminal_dispatch_error(&error) => {
                tracing::error!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error = %error,
                    "acknowledging terminal outbound notification dispatch error"
                );
                pending.ack().await?;
                Ok(OutboundNotificationConsumerAction::Acknowledged)
            }
            Err(error) => {
                tracing::warn!(
                    event_id,
                    delivery_id = %delivery.id(),
                    error = %error,
                    "leaving outbound notification fact unacknowledged after infrastructure failure"
                );
                drop(pending);
                Ok(OutboundNotificationConsumerAction::DeferredToEventProvider)
            }
        }
    }
}

fn terminal_dispatch_error(error: &ApplicationError) -> bool {
    matches!(
        error,
        ApplicationError::Invalid(_)
            | ApplicationError::NotFound(_)
            | ApplicationError::Conflict(_)
            | ApplicationError::Forbidden(_)
            | ApplicationError::Unavailable(_)
    )
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
        || event.version != 1
        || event.source != "a3s-cloud"
        || received.num_delivered == 0
    {
        return Err("outbound notification event envelope is invalid".into());
    }
    let envelope: PublishedOutboxEnvelope = serde_json::from_value(event.payload.clone())
        .map_err(|_| "outbound notification Outbox envelope is invalid".to_owned())?;
    let delivery = OutboundNotificationDelivery::from_payload(&envelope.data)?;
    if envelope.organization_id != delivery.organization_id().as_uuid()
        || envelope.aggregate_id != delivery.id()
        || envelope.aggregate_version != 1
        || envelope.occurred_at != delivery.occurred_at()
        || envelope.correlation_id != delivery.correlation_id()
        || envelope.causation_id != Some(delivery.source_event_id())
        || event_id != delivery.requested_event_id()
    {
        return Err("outbound notification fact identity is inconsistent".into());
    }
    Ok(delivery)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublishedOutboxEnvelope {
    organization_id: Uuid,
    aggregate_id: Uuid,
    aggregate_version: u64,
    occurred_at: DateTime<Utc>,
    correlation_id: Uuid,
    causation_id: Option<Uuid>,
    data: serde_json::Value,
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
