use super::{AutomationInvocationAdmissionOutcome, IAutomationInvocationAdmission};
use crate::modules::automations::domain::IAutomationInvocationRepository;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::AutomationInvocationEnvelopeV1;
use async_trait::async_trait;
use std::sync::Arc;

/// Application boundary for admitting one exact invocation envelope.
///
/// The repository owns idempotent persistence and transactional side effects.
/// This service validates the contract before crossing that boundary and maps
/// an exact replay to `AlreadyAdmitted` for schedule/event redelivery.
#[derive(Clone)]
pub struct AutomationInvocationAdmissionService {
    repository: Arc<dyn IAutomationInvocationRepository>,
}

impl AutomationInvocationAdmissionService {
    pub fn new(repository: Arc<dyn IAutomationInvocationRepository>) -> Self {
        Self { repository }
    }

    pub async fn admit_invocation(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome> {
        envelope.validate().map_err(ApplicationError::Invalid)?;
        let admission = self.repository.admit(envelope).await?;
        Ok(if admission.replayed {
            AutomationInvocationAdmissionOutcome::AlreadyAdmitted
        } else {
            AutomationInvocationAdmissionOutcome::Admitted
        })
    }
}

#[async_trait]
impl IAutomationInvocationAdmission for AutomationInvocationAdmissionService {
    async fn admit(
        &self,
        envelope: AutomationInvocationEnvelopeV1,
    ) -> ApplicationResult<AutomationInvocationAdmissionOutcome> {
        self.admit_invocation(envelope).await
    }
}
