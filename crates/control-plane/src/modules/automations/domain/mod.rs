mod concurrency;
mod due;
mod entities;
mod lease;
mod misfire;
mod repositories;
mod schedule;
mod schedule_state;
mod services;

pub use concurrency::{AutomationConcurrencyDecision, AutomationConcurrencyEvaluator};
pub use due::{AutomationScheduleDueEvaluation, AutomationScheduleDueEvaluator};
pub use entities::{AutomationWebhookDeliveryRecord, AutomationWebhookEndpointRecord};
pub use lease::{
    AutomationScheduleLease, AutomationScheduleLeaseEvaluationRequest,
    AutomationScheduleLeaseEvaluator, AUTOMATION_SCHEDULE_MAX_LEASE_MS,
};
pub use misfire::{AutomationScheduleDueSelection, AutomationScheduleMisfireEvaluator};
pub use repositories::{
    AdmitAutomationWebhookDeliveryWrite, AutomationWebhookAdmission, EndpointLifecycleAction,
    IAutomationWebhookRepository, TransitionAutomationWebhookEndpoint,
};
pub use schedule::{AutomationScheduleCalculator, AUTOMATION_SCHEDULE_MAX_OCCURRENCES};
pub use schedule_state::{
    AutomationScheduleState, AutomationScheduleStateKey, CommitAutomationScheduleCursor,
    IAutomationScheduleStateRepository, ReserveAutomationScheduleLease,
};
pub use services::{IAutomationWebhookSchemaValidator, IAutomationWebhookSignatureVerifier};
