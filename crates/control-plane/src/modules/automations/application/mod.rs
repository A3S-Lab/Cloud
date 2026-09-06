mod event_consumer;
mod event_dispatch;
mod event_fanout;
mod event_invocation;
mod invocation_admission;
mod schedule_dispatch;
mod webhook_admission;

#[cfg(test)]
mod tests;

pub use crate::modules::automations::domain::EndpointLifecycleAction;
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
pub use schedule_dispatch::{
    AutomationScheduleDispatchRequest, AutomationScheduleDispatchResult,
    AutomationScheduleDispatchService, IAutomationScheduleDispatchService,
};
pub use webhook_admission::{
    AdmitAutomationWebhookDelivery, AutomationWebhookAdmissionService,
    ChangeAutomationWebhookEndpoint, CreateAutomationWebhookEndpoint,
};
