mod in_memory;
mod outbound_connector;
mod outbox_projector;
mod postgres;

pub use in_memory::InMemoryNotificationRepository;
pub use outbound_connector::{
    SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter,
};
pub use outbox_projector::OutboxNotificationProjector;
pub use postgres::PostgresNotificationRepository;
