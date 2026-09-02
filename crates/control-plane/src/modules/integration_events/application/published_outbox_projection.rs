use crate::modules::integration_events::domain::entities::OutboxMessage;
use crate::modules::integration_events::published::PublishedOutboxEnvelope;

/// Maps one validated committed fact into the bounded Integration Events Published Language.
pub fn project_published_outbox_envelope(
    message: &OutboxMessage,
) -> Result<PublishedOutboxEnvelope, String> {
    let event = message.domain_event()?;
    PublishedOutboxEnvelope::from_committed_event(message.scope, event)
}
