use crate::modules::automations::application::event_consumer::IAutomationNormalizedEventHandler;
use crate::modules::automations::application::event_fanout::{
    AutomationEventInvocationCandidate, AutomationEventInvocationFanoutService,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationEnvelopeV1,
    AutomationNormalizedEventV1, AutomationRevisionV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// Owned candidate returned by an external subscription/revision authority.
///
/// The provider owns candidate discovery and the durable revision projection;
/// Automations only borrows these values while evaluating one event.
#[derive(Debug, Clone)]
pub struct AutomationEventInvocationCandidateOwned {
    pub revision: AutomationRevisionV1,
    pub invocation_id: Uuid,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

impl AutomationEventInvocationCandidateOwned {
    fn as_borrowed(&self) -> AutomationEventInvocationCandidate<'_> {
        AutomationEventInvocationCandidate {
            revision: &self.revision,
            invocation_id: self.invocation_id,
            authorization: self.authorization.clone(),
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
        }
    }
}

/// External owner of the bounded candidate set for one normalized event.
///
/// Implementations may query a durable revision/subscription projection, but
/// they retain ownership of that storage and its authorization rules.
#[async_trait]
pub trait IAutomationEventCandidateProvider: Send + Sync {
    async fn candidates(
        &self,
        event: &AutomationNormalizedEventV1,
    ) -> ApplicationResult<Vec<AutomationEventInvocationCandidateOwned>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationInvocationAdmissionOutcome {
    Admitted,
    AlreadyAdmitted,
}

/// Atomic/idempotent owner of an exact invocation envelope.
///
/// The implementation persists or forwards the envelope under its own
/// authority. Replays must return `AlreadyAdmitted` instead of repeating a
/// target-side effect.
#[async_trait]
pub trait IAutomationInvocationAdmission: Send + Sync {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome>;
}

/// Composes external candidate selection, deterministic event fan-out, and
/// idempotent invocation admission.
///
/// This service does not own candidate storage, subscription persistence,
/// invocation tables, queues, retries, or target execution. A failure after a
/// partial admission is safe only when the admission port is idempotent for
/// the exact envelope, allowing the provider to redeliver the event.
#[derive(Clone)]
pub struct AutomationEventInvocationDispatchService {
    candidates: Arc<dyn IAutomationEventCandidateProvider>,
    fanout: AutomationEventInvocationFanoutService,
    admission: Arc<dyn IAutomationInvocationAdmission>,
}

impl AutomationEventInvocationDispatchService {
    pub fn new(
        candidates: Arc<dyn IAutomationEventCandidateProvider>,
        fanout: AutomationEventInvocationFanoutService,
        admission: Arc<dyn IAutomationInvocationAdmission>,
    ) -> Self {
        Self {
            candidates,
            fanout,
            admission,
        }
    }

    pub async fn dispatch_at(
        &self,
        event: &AutomationNormalizedEventV1,
        requested_at: DateTime<Utc>,
    ) -> ApplicationResult<usize> {
        event.validate().map_err(ApplicationError::Invalid)?;
        let owned = self.candidates.candidates(event).await?;
        let borrowed = owned
            .iter()
            .map(AutomationEventInvocationCandidateOwned::as_borrowed)
            .collect();
        let envelopes = self.fanout.evaluate(event, requested_at, borrowed).await?;
        let mut processed = 0;
        for envelope in envelopes {
            self.admission.admit(envelope).await?;
            processed += 1;
        }
        Ok(processed)
    }
}

#[async_trait]
impl IAutomationNormalizedEventHandler for AutomationEventInvocationDispatchService {
    async fn handle(&self, event: AutomationNormalizedEventV1) -> Result<(), String> {
        // The observed event timestamp is immutable across provider redelivery.
        // Using wall-clock time here would drift the complete envelope and
        // defeat exact admission replay.
        self.dispatch_at(&event, event.observed_at)
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[path = "event_dispatch_tests.rs"]
mod tests;
