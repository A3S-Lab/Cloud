mod alert_policy_postgres;
mod fleet_node_access;
mod in_memory;
mod outbound_connector;
mod outbound_event_consumer;
mod outbound_postgres;
mod outbound_smtp;
mod outbound_smtp_in_memory;
mod outbound_smtp_postgres;
mod outbound_recipient_contact_access;
mod outbox_identity_access;
mod outbox_projector;
mod postgres;
mod project_environment_access;

pub use fleet_node_access::FleetNotificationsNodeAccessAdapter;
pub use in_memory::InMemoryNotificationRepository;
pub use outbound_connector::{
    SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter,
};
pub use outbound_event_consumer::{
    A3sEventOutboundNotificationConsumer, OutboundNotificationConsumerAction,
    OUTBOUND_NOTIFICATION_SUBSCRIBER_ID,
};
pub use outbound_recipient_contact_access::IdentityOutboundRecipientContactAccessAdapter;
pub use outbound_smtp::{
    SmtpOutboundNotificationCredentials, SmtpOutboundNotificationDeliveryOptions,
    SmtpOutboundNotificationDeliveryService, SmtpOutboundNotificationTlsPolicy,
};
pub use outbox_identity_access::IdentityNotificationOutboxIdentityAccessAdapter;
pub use outbox_projector::OutboxNotificationProjector;
pub use postgres::PostgresNotificationRepository;
pub use project_environment_access::ProjectsNotificationsEnvironmentAccessAdapter;
