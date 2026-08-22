mod alert_policy_postgres;
mod in_memory;
mod outbound_connector;
mod outbound_event_consumer;
mod outbound_postgres;
mod outbound_smtp;
mod outbound_smtp_in_memory;
mod outbound_smtp_postgres;
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
pub use outbound_smtp::{
    SmtpOutboundNotificationCredentials, SmtpOutboundNotificationDeliveryOptions,
    SmtpOutboundNotificationDeliveryService, SmtpOutboundNotificationTlsPolicy,
};
pub use outbox_projector::OutboxNotificationProjector;
pub use postgres::PostgresNotificationRepository;
