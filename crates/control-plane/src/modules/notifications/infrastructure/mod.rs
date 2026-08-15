mod in_memory;
mod outbound_connector;
mod outbound_event_consumer;
mod outbox_projector;
mod postgres;

pub use in_memory::InMemoryNotificationRepository;
pub use outbound_connector::{
    SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter,
};
pub use outbound_event_consumer::{
    A3sEventOutboundNotificationConsumer, OutboundNotificationConsumerAction,
    OUTBOUND_NOTIFICATION_SUBSCRIBER_ID,
};
pub use outbox_projector::OutboxNotificationProjector;
pub use postgres::PostgresNotificationRepository;
