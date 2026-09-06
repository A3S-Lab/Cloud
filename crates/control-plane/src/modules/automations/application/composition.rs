use super::{
    AutomationEventInvocationDispatchService, AutomationEventInvocationEvaluationService,
    AutomationEventInvocationFanoutService, AutomationInvocationAdmissionService,
    AutomationScheduleDispatchService, IAutomationEventCandidateProvider,
    IAutomationInvocationAdmission,
};
use crate::modules::automations::domain::{
    IAutomationEventFilterEvaluator, IAutomationInvocationRepository,
    IAutomationScheduleStateRepository,
};
use std::sync::Arc;

/// The application composition boundary for all new-invocation paths.
///
/// Schedule and normalized-event dispatch receive the same admission port and
/// therefore cannot accidentally create parallel invocation stores or Outbox
/// writers. Candidate discovery, filter evaluation, schedule state, and
/// persistence remain injected owner ports.
#[derive(Clone)]
pub struct AutomationsDispatchServices {
    pub admission: Arc<AutomationInvocationAdmissionService>,
    pub schedule: AutomationScheduleDispatchService,
    pub event: AutomationEventInvocationDispatchService,
}

impl AutomationsDispatchServices {
    pub fn new(
        invocations: Arc<dyn IAutomationInvocationRepository>,
        schedule_states: Arc<dyn IAutomationScheduleStateRepository>,
        candidates: Arc<dyn IAutomationEventCandidateProvider>,
        filters: Arc<dyn IAutomationEventFilterEvaluator>,
    ) -> Self {
        let admission = Arc::new(AutomationInvocationAdmissionService::new(invocations));
        let admission_port: Arc<dyn IAutomationInvocationAdmission> = admission.clone();
        let evaluation = AutomationEventInvocationEvaluationService::new(filters);
        let fanout = AutomationEventInvocationFanoutService::new(evaluation);
        Self {
            admission,
            schedule: AutomationScheduleDispatchService::new(
                schedule_states,
                Arc::clone(&admission_port),
            ),
            event: AutomationEventInvocationDispatchService::new(
                candidates,
                fanout,
                admission_port,
            ),
        }
    }
}
