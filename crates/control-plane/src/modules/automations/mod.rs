//! Automations owns new-invocation admission state.
//!
//! This module currently exposes the AUT0.2 component boundary only.  It does
//! not register an HTTP listener, Gateway route, worker, or public management
//! surface.  Those integrations must consume the application ports below
//! rather than copying webhook or invocation state into another context.

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint, EndpointLifecycleAction,
};
pub use domain::{
    AutomationScheduleCalculator, AutomationScheduleDueSelection,
    AutomationScheduleMisfireEvaluator, AutomationWebhookAdmission,
    AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord, IAutomationWebhookRepository,
    IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier,
    TransitionAutomationWebhookEndpoint, AUTOMATION_SCHEDULE_MAX_OCCURRENCES,
};
pub use infrastructure::{
    DigestBoundJsonSchemaValidator, HmacSha256AutomationWebhookSignatureVerifier,
    InMemoryAutomationWebhookRepository, PostgresAutomationWebhookRepository,
    AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
};
