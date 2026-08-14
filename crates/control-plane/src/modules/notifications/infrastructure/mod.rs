mod in_memory;
mod outbound_http;
mod outbox_projector;
mod postgres;

pub use in_memory::InMemoryNotificationRepository;
pub use outbound_http::{SignedWebhookNotificationAdapter, SlackCompatibleNotificationAdapter};
pub use outbox_projector::OutboxNotificationProjector;
pub use postgres::PostgresNotificationRepository;
