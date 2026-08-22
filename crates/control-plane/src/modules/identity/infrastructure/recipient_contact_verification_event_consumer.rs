use crate::modules::identity::application::{
    IRecipientContactVerificationDispatcher, RecipientContactVerificationDispatchResult,
};
use crate::modules::identity::domain::entities::{
    RecipientContactVerification, RecipientContactVerificationDeliveryFact,
};
use crate::modules::identity::domain::events::RecipientContactChanged;
use crate::modules::identity::domain::value_objects::RecipientContactSigningKeyId;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId, RecipientContactId};
use a3s_event::{
    DeliverPolicy, EventBus, EventError, PendingEvent, ReceivedEvent, SubscribeOptions,
    SubscriptionFilter,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

pub const RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY: &str =
    "identity.recipient-contact.verification-requested";
pub const RECIPIENT_CONTACT_VERIFICATION_DELIVERY_SUBSCRIBER_ID: &str =
    "a3s-cloud-identity-recipient-contact-verification-delivery-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientContactVerificationConsumerAction {
    Acknowledged,
    DeferredToEventProvider,
}

pub struct A3sEventRecipientContactVerificationConsumer {
    bus: Arc<EventBus>,
    subject: String,
    dispatcher: Arc<dyn IRecipientContactVerificationDispatcher>,
}

impl A3sEventRecipientContactVerificationConsumer {
    pub fn new(
        bus: Arc<EventBus>,
        subject: impl Into<String>,
        dispatcher: Arc<dyn IRecipientContactVerificationDispatcher>,
    ) -> Result<Self, String> {
        let subject = subject.into();
        if !valid_exact_subject(&subject) {
            return Err("recipient contact verification A3S Event subject is invalid".into());
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
                subscriber_id: RECIPIENT_CONTACT_VERIFICATION_DELIVERY_SUBSCRIBER_ID.into(),
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
            .create_subscriber(RECIPIENT_CONTACT_VERIFICATION_DELIVERY_SUBSCRIBER_ID)
            .await?;
        if subscriptions.len() != 1 {
            return Err(EventError::Config(
                "recipient contact verification consumer requires one exact subscription".into(),
            ));
        }
        let mut subscription = subscriptions.pop().ok_or_else(|| {
            EventError::Config("recipient contact verification subscription is missing".into())
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
                        "recipient contact verification subscription ended before shutdown".into()
                    ))?;
                    self.process_pending(pending).await?;
                }
            }
        }
    }

    async fn process_pending(
        &self,
        pending: PendingEvent,
    ) -> a3s_event::Result<RecipientContactVerificationConsumerAction> {
        let event_id = pending.received.event.id.clone();
        let fact = match decode_verification_requested_event(&pending.received, &self.subject) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(
                    event_id,
                    error,
                    "acknowledging malformed recipient contact verification fact"
                );
                pending.ack().await?;
                return Ok(RecipientContactVerificationConsumerAction::Acknowledged);
            }
        };
        match self.dispatcher.dispatch(&fact).await {
            Ok(RecipientContactVerificationDispatchResult::Terminal(status)) => {
                tracing::debug!(
                    event_id,
                    verification_id = %fact.id(),
                    status = status.as_str(),
                    "acknowledging terminal recipient contact verification delivery"
                );
                pending.ack().await?;
                Ok(RecipientContactVerificationConsumerAction::Acknowledged)
            }
            Ok(RecipientContactVerificationDispatchResult::InvalidFact) => {
                tracing::warn!(
                    event_id,
                    verification_id = %fact.id(),
                    "acknowledging recipient contact verification fact rejected by Identity authority"
                );
                pending.ack().await?;
                Ok(RecipientContactVerificationConsumerAction::Acknowledged)
            }
            Ok(RecipientContactVerificationDispatchResult::Deferred) | Err(_) => {
                tracing::warn!(
                    event_id,
                    verification_id = %fact.id(),
                    "leaving recipient contact verification fact unacknowledged for provider-owned redelivery"
                );
                drop(pending);
                Ok(RecipientContactVerificationConsumerAction::DeferredToEventProvider)
            }
        }
    }
}

fn decode_verification_requested_event(
    received: &ReceivedEvent,
    expected_subject: &str,
) -> Result<RecipientContactVerificationDeliveryFact, String> {
    let event = &received.event;
    let event_id = Uuid::parse_str(&event.id)
        .ok()
        .filter(|value| !value.is_nil())
        .ok_or_else(|| "recipient contact verification event ID is invalid".to_owned())?;
    if event.subject != expected_subject
        || event.category != "cloud"
        || event.event_type != RECIPIENT_CONTACT_VERIFICATION_REQUESTED_EVENT_KEY
        || event.source != "a3s-cloud"
        || event.version != 1
        || received.num_delivered == 0
    {
        return Err("recipient contact verification event envelope is invalid".into());
    }
    let envelope: PublishedOutboxEnvelope = serde_json::from_value(event.payload.clone())
        .map_err(|_| "recipient contact verification Outbox envelope is invalid".to_owned())?;
    let payload: RecipientContactChanged = serde_json::from_value(envelope.data)
        .map_err(|_| "recipient contact verification payload is invalid".to_owned())?;
    let challenge_id = payload
        .challenge_id
        .ok_or_else(|| "recipient contact verification challenge ID is missing".to_owned())?;
    let signing_key_id = RecipientContactSigningKeyId::parse(
        payload
            .signing_key_id
            .ok_or_else(|| "recipient contact verification signing key is missing".to_owned())?,
    )?;
    let issued_at = payload
        .challenge_issued_at
        .ok_or_else(|| "recipient contact verification issue time is missing".to_owned())?;
    let expires_at = payload
        .challenge_expires_at
        .ok_or_else(|| "recipient contact verification expiry is missing".to_owned())?;
    let verification = RecipientContactVerification::create(
        challenge_id,
        RecipientContactId::from_uuid(payload.contact_id),
        PrincipalId::from_uuid(payload.principal_id),
        payload.address_digest,
        payload.contact_version,
        signing_key_id,
        issued_at,
        expires_at,
    )?;
    if payload.state != "pending"
        || payload.verified_at.is_some()
        || payload.revoked_at.is_some()
        || event_id != challenge_id.as_uuid()
        || envelope.organization_id.is_nil()
        || envelope.aggregate_id != payload.contact_id
        || envelope.aggregate_version != payload.contact_version
        || envelope.occurred_at != issued_at
        || envelope.correlation_id.is_nil()
        || envelope.causation_id.is_some()
    {
        return Err("recipient contact verification fact identity is inconsistent".into());
    }
    let fact = RecipientContactVerificationDeliveryFact {
        organization_id: OrganizationId::from_uuid(envelope.organization_id),
        verification,
    };
    fact.validate()?;
    Ok(fact)
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
#[path = "recipient_contact_verification_event_consumer_tests.rs"]
mod tests;
