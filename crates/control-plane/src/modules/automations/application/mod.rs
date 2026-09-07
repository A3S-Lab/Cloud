mod composition;
mod definition_catalog;
mod endpoint_query;
mod event_consumer;
mod event_dispatch;
mod event_fanout;
mod event_invocation;
mod invocation_admission;
mod invocation_consumer;
mod schedule_candidates;
mod schedule_dispatch;
mod schedule_worker;
mod webhook_admission;
mod webhook_transport;

#[cfg(test)]
mod tests;

pub use crate::modules::automations::domain::EndpointLifecycleAction;
pub use composition::AutomationsDispatchServices;
pub use definition_catalog::AutomationDefinitionCatalogService;
pub use endpoint_query::{
    AutomationWebhookEndpointQueryService, AutomationWebhookEndpointScope,
    ResolveAutomationWebhookEndpoint,
};
pub use event_consumer::IAutomationNormalizedEventHandler;
pub use event_dispatch::{
    AutomationEventInvocationCandidateOwned, AutomationEventInvocationDispatchService,
    AutomationInvocationAdmissionOutcome, IAutomationEventCandidateProvider,
    IAutomationInvocationAdmission,
};
pub use event_fanout::{
    AutomationEventInvocationCandidate, AutomationEventInvocationFanoutService,
    AUTOMATION_MAX_EVENT_FANOUT_CANDIDATES,
};
pub use event_invocation::AutomationEventInvocationEvaluationService;
pub use invocation_admission::AutomationInvocationAdmissionService;
pub use invocation_consumer::IAutomationInvocationHandler;
pub use schedule_candidates::RepositoryAutomationScheduleCandidateProvider;
pub use schedule_dispatch::{
    AutomationScheduleDispatchRequest, AutomationScheduleDispatchResult,
    AutomationScheduleDispatchService, IAutomationScheduleDispatchService,
};
pub use schedule_worker::{
    AutomationScheduleCandidate, AutomationScheduleWorker, AutomationScheduleWorkerConfig,
    AutomationScheduleWorkerReport, IAutomationScheduleCandidateProvider,
};
pub use webhook_admission::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint,
};
pub use webhook_transport::{AutomationWebhookReceiver, ReceiveAutomationWebhookDelivery};
