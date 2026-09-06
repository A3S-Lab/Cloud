//! Automations owns new-invocation admission state.
//!
//! This module exposes the AUT0.2 admission and AUT0.3 schedule/invocation
//! component boundaries. It does not register an HTTP listener, Gateway route,
//! production candidate provider, or public management surface. The schedule
//! worker is an injectable timer boundary only; process registration and owner
//! composition must consume these ports rather than copying webhook,
//! invocation, cursor, or lease state into another context.

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod presentation;

pub use application::{
    AdmitAutomationWebhookDelivery, AutomationEventInvocationCandidate,
    AutomationEventInvocationCandidateOwned, AutomationEventInvocationDispatchService,
    AutomationEventInvocationEvaluationService, AutomationEventInvocationFanoutService,
    AutomationInvocationAdmissionOutcome, AutomationInvocationAdmissionService,
    AutomationScheduleCandidate, AutomationScheduleDispatchRequest,
    AutomationScheduleDispatchResult, AutomationScheduleDispatchService, AutomationScheduleWorker,
    AutomationScheduleWorkerConfig, AutomationScheduleWorkerReport,
    AutomationWebhookAdmissionService, AutomationWebhookEndpointQueryService,
    AutomationWebhookEndpointScope, AutomationWebhookReceiver, AutomationsDispatchServices,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint, EndpointLifecycleAction,
    IAutomationEventCandidateProvider, IAutomationInvocationAdmission,
    IAutomationInvocationHandler, IAutomationNormalizedEventHandler,
    IAutomationScheduleCandidateProvider, IAutomationScheduleDispatchService,
    ReceiveAutomationWebhookDelivery, ResolveAutomationWebhookEndpoint,
    AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES,
};
pub use domain::{
    AutomationConcurrencyDecision, AutomationConcurrencyEvaluator,
    AutomationEventInvocationFactory, AutomationEventInvocationRequest,
    AutomationInvocationAdmission, AutomationInvocationRecord, AutomationScheduleCalculator,
    AutomationScheduleDueEvaluation, AutomationScheduleDueEvaluator,
    AutomationScheduleDueSelection, AutomationScheduleInvocationFactory,
    AutomationScheduleInvocationRequest, AutomationScheduleLease,
    AutomationScheduleLeaseEvaluationRequest, AutomationScheduleLeaseEvaluator,
    AutomationScheduleMisfireEvaluator, AutomationScheduleState, AutomationScheduleStateKey,
    AutomationWebhookAdmission, AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord,
    AutomationWebhookInvocationFactory, AutomationWebhookInvocationRequest,
    CommitAutomationScheduleCursor, IAutomationEventFilterEvaluator, IAutomationInvocationReader,
    IAutomationInvocationRepository, IAutomationScheduleStateRepository,
    IAutomationWebhookRepository, IAutomationWebhookSchemaRegistry,
    IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier,
    ReleaseAutomationScheduleLease, ReserveAutomationScheduleLease,
    TransitionAutomationWebhookEndpoint, AUTOMATION_SCHEDULE_MAX_LEASE_MS,
    AUTOMATION_SCHEDULE_MAX_OCCURRENCES,
};
pub use infrastructure::{
    A3sEventAutomationInvocationConsumer, A3sEventAutomationNormalizedEventConsumer,
    AutomationEventConsumerAction, AutomationInvocationConsumerAction,
    DigestBoundJsonSchemaValidator, HmacSha256AutomationWebhookSignatureVerifier,
    InMemoryAutomationInvocationRepository, InMemoryAutomationScheduleStateRepository,
    InMemoryAutomationWebhookRepository, InMemoryAutomationWebhookSchemaRegistry,
    PostgresAutomationInvocationRepository, PostgresAutomationScheduleStateRepository,
    PostgresAutomationWebhookRepository, RegistryBackedAutomationWebhookSchemaValidator,
    AUTOMATION_INVOCATION_ADMITTED_EVENT_KEY, AUTOMATION_INVOCATION_ADMITTED_SOURCE,
    AUTOMATION_INVOCATION_ADMITTED_SUBJECT, AUTOMATION_INVOCATION_SUBSCRIBER_ID,
    AUTOMATION_NORMALIZED_EVENT_SUBSCRIBER_ID, AUTOMATION_WEBHOOK_SCHEMA_MAX_BYTES,
};
