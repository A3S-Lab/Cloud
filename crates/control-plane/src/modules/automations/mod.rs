//! Automations owns new-invocation admission state.
//!
//! This module exposes the AUT0.2 admission and AUT0.3 schedule/invocation
//! component boundaries. It does not register an HTTP listener, Gateway route,
//! scheduler, worker, or public management surface. Those integrations must
//! consume the application ports below rather than copying webhook, invocation,
//! cursor, or lease state into another context.

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    AdmitAutomationWebhookDelivery, AutomationEventInvocationCandidate,
    AutomationEventInvocationEvaluationService, AutomationEventInvocationFanoutService,
    AutomationWebhookAdmissionService, ChangeAutomationWebhookEndpoint,
    CreateAutomationWebhookEndpoint, EndpointLifecycleAction,
    AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES,
};
pub use domain::{
    AutomationConcurrencyDecision, AutomationConcurrencyEvaluator,
    AutomationEventInvocationFactory, AutomationEventInvocationRequest,
    AutomationScheduleCalculator, AutomationScheduleDueEvaluation, AutomationScheduleDueEvaluator,
    AutomationScheduleDueSelection, AutomationScheduleInvocationFactory,
    AutomationScheduleInvocationRequest, AutomationScheduleLease,
    AutomationScheduleLeaseEvaluationRequest, AutomationScheduleLeaseEvaluator,
    AutomationScheduleMisfireEvaluator, AutomationScheduleState, AutomationScheduleStateKey,
    AutomationWebhookAdmission, AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord,
    CommitAutomationScheduleCursor, IAutomationEventFilterEvaluator,
    IAutomationScheduleStateRepository, IAutomationWebhookRepository,
    IAutomationWebhookSchemaRegistry, IAutomationWebhookSchemaValidator,
    IAutomationWebhookSignatureVerifier, ReserveAutomationScheduleLease,
    TransitionAutomationWebhookEndpoint, AUTOMATION_SCHEDULE_MAX_LEASE_MS,
    AUTOMATION_SCHEDULE_MAX_OCCURRENCES,
};
pub use infrastructure::{
    DigestBoundJsonSchemaValidator, HmacSha256AutomationWebhookSignatureVerifier,
    InMemoryAutomationScheduleStateRepository, InMemoryAutomationWebhookRepository,
    InMemoryAutomationWebhookSchemaRegistry, PostgresAutomationScheduleStateRepository,
    PostgresAutomationWebhookRepository, RegistryBackedAutomationWebhookSchemaValidator,
    AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
};
