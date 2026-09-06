mod event_consumer;
mod in_memory;
mod invocation_consumer;
mod invocation_in_memory;
mod invocation_postgres;
mod postgres;
mod schedule_state_in_memory;
mod schedule_state_postgres;
mod schema;
mod schema_registry;
mod signature;

pub use event_consumer::{
    A3sEventAutomationNormalizedEventConsumer, AutomationEventConsumerAction,
    AUTOMATION_NORMALIZED_EVENT_SUBSCRIBER_ID,
};
pub use in_memory::InMemoryAutomationWebhookRepository;
pub use invocation_consumer::{
    A3sEventAutomationInvocationConsumer, AutomationInvocationConsumerAction,
    AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY, AUTOMATION_INVOCATION_ADMITTED_SOURCE,
    AUTOMATION_INVOCATION_ADMITTED_SUBJECT, AUTOMATION_INVOCATION_SUBSCRIBER_ID,
};
pub use invocation_in_memory::InMemoryAutomationInvocationRepository;
pub use invocation_postgres::PostgresAutomationInvocationRepository;
pub use postgres::PostgresAutomationWebhookRepository;
pub use schedule_state_in_memory::InMemoryAutomationScheduleStateRepository;
pub use schedule_state_postgres::PostgresAutomationScheduleStateRepository;
pub use schema::{DigestBoundJsonSchemaValidator, AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES};
pub use schema_registry::{
    InMemoryAutomationWebhookSchemaRegistry, RegistryBackedAutomationWebhookSchemaValidator,
};
pub use signature::HmacSha256AutomationWebhookSignatureVerifier;
