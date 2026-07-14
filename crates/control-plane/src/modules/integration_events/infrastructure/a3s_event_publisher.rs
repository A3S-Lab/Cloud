use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::integration_events::domain::services::{EventPublishError, IEventPublisher};
use a3s_event::{Event, EventBus, MemoryProvider, NatsConfig, NatsProvider, PublishOptions};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

pub struct A3sEventPublisher {
    bus: Arc<EventBus>,
    subject_prefix: String,
}

impl A3sEventPublisher {
    pub fn memory() -> Self {
        Self::from_bus(EventBus::new(MemoryProvider::default()))
    }

    pub async fn nats(config: NatsConfig) -> Result<Self, EventPublishError> {
        let subject_prefix = config.subject_prefix.clone();
        let provider = NatsProvider::connect(config)
            .await
            .map_err(|error| EventPublishError::new(error.to_string()))?;
        Ok(Self::from_bus_with_subject_prefix(
            EventBus::new(provider),
            subject_prefix,
        ))
    }

    pub fn from_bus(bus: EventBus) -> Self {
        Self::from_bus_with_subject_prefix(bus, "events")
    }

    pub fn from_bus_with_subject_prefix(bus: EventBus, subject_prefix: impl Into<String>) -> Self {
        Self {
            bus: Arc::new(bus),
            subject_prefix: subject_prefix.into(),
        }
    }

    pub fn bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.bus)
    }
}

#[async_trait]
impl IEventPublisher for A3sEventPublisher {
    async fn publish(&self, message: &OutboxMessage) -> Result<(), EventPublishError> {
        let mut event = Event::typed(
            format!("{}.cloud.{}", self.subject_prefix, message.event_key),
            "cloud",
            &message.event_key,
            message.schema_version,
            &message.event_key,
            "a3s-cloud",
            json!({
                "organizationId": message.organization_id,
                "aggregateId": message.aggregate_id,
                "aggregateVersion": message.aggregate_version,
                "occurredAt": message.occurred_at,
                "correlationId": message.correlation_id,
                "causationId": message.causation_id,
                "data": message.payload,
            }),
        );
        event.id = message.event_id.to_string();
        self.bus
            .publish_event_with_options(
                &event,
                &PublishOptions {
                    msg_id: Some(message.event_id.to_string()),
                    ..PublishOptions::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| EventPublishError::new(error.to_string()))
    }

    async fn health(&self) -> Result<bool, EventPublishError> {
        self.bus
            .health()
            .await
            .map_err(|error| EventPublishError::new(error.to_string()))
    }
}
