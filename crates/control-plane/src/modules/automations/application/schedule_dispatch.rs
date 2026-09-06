use crate::modules::automations::application::event_dispatch::{
    AutomationInvocationAdmissionOutcome, IAutomationInvocationAdmission,
};
use crate::modules::automations::domain::{
    AutomationScheduleCalculator, AutomationScheduleInvocationFactory,
    AutomationScheduleInvocationRequest, AutomationScheduleLeaseEvaluationRequest,
    AutomationScheduleLeaseEvaluator, AutomationScheduleStateKey, CommitAutomationScheduleCursor,
    IAutomationScheduleStateRepository, ReleaseAutomationScheduleLease,
    ReserveAutomationScheduleLease,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationInputV1, AutomationRevisionV1,
    AutomationTriggerV1,
};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use std::sync::Arc;
use uuid::Uuid;

/// Input for one bounded schedule-owner evaluation.
pub struct AutomationScheduleDispatchRequest {
    pub key: AutomationScheduleStateKey,
    pub revision: AutomationRevisionV1,
    pub owner_id: Uuid,
    pub lease_id: Uuid,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub limit: usize,
    /// Immutable input authority for this exact schedule revision. The owner
    /// must resolve the same value on every redelivery of the occurrence.
    pub input: AutomationInvocationInputV1,
    /// Immutable authorization snapshot for this exact schedule revision.
    pub authorization: AutomationInvocationAuthorizationV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationScheduleDispatchResult {
    pub admitted: usize,
    pub already_admitted: usize,
    pub skipped: usize,
    pub evaluated_through: Option<DateTime<Utc>>,
}

/// Owns one lease-guarded, bounded schedule evaluation and hands exact
/// envelopes to the shared idempotent admission port.
///
/// The service deliberately does not run a timer loop, discover revisions,
/// persist definitions, enqueue work, or execute targets. A worker can call it
/// periodically; durable cursor progress and admission replay make process
/// death safe without a second scheduler authority.
#[derive(Clone)]
pub struct AutomationScheduleDispatchService {
    states: Arc<dyn IAutomationScheduleStateRepository>,
    admission: Arc<dyn IAutomationInvocationAdmission>,
}

impl AutomationScheduleDispatchService {
    pub fn new(
        states: Arc<dyn IAutomationScheduleStateRepository>,
        admission: Arc<dyn IAutomationInvocationAdmission>,
    ) -> Self {
        Self { states, admission }
    }

    pub async fn dispatch(
        &self,
        request: AutomationScheduleDispatchRequest,
    ) -> ApplicationResult<AutomationScheduleDispatchResult> {
        request
            .revision
            .validate()
            .map_err(ApplicationError::Invalid)?;
        if request.owner_id.is_nil() || request.lease_id.is_nil() {
            return Err(ApplicationError::Invalid(
                "Automation schedule dispatch identity is invalid".into(),
            ));
        }
        if request.observed_at < request.reserved_at {
            return Err(ApplicationError::Invalid(
                "Automation schedule observation cannot precede lease reservation".into(),
            ));
        }

        let definition = &request.revision.spec().definition;
        let AutomationTriggerV1::Schedule(trigger) = &definition.trigger else {
            return Err(ApplicationError::Invalid(
                "Automation schedule dispatch requires a schedule revision".into(),
            ));
        };
        request
            .input
            .validate()
            .map_err(ApplicationError::Invalid)?;
        request
            .authorization
            .validate()
            .map_err(ApplicationError::Invalid)?;
        if request.authorization.policy_digest != definition.authorization.policy_digest {
            return Err(ApplicationError::Conflict(
                "Automation schedule authorization policy drifted".into(),
            ));
        }
        if request.limit == 0
            || request.limit
                > crate::modules::automations::domain::AUTOMATION_SCHEDULE_MAX_OCCURRENCES
        {
            return Err(ApplicationError::Invalid(
                "Automation schedule occurrence limit is outside its bound".into(),
            ));
        }
        let calculator =
            AutomationScheduleCalculator::new(trigger).map_err(ApplicationError::Invalid)?;
        if request.key.automation_id != definition.automation_id
            || request.key.organization_id.as_uuid() != definition.organization_id
            || request.key.project_id.as_uuid() != definition.project_id
            || request.key.environment_id.as_uuid() != definition.environment_id
        {
            return Err(ApplicationError::Conflict(
                "Automation schedule state scope does not match the exact revision".into(),
            ));
        }

        let state = self.states.find(request.key).await?.ok_or_else(|| {
            ApplicationError::NotFound("automation schedule state not found".into())
        })?;
        if state.revision_id() != request.revision.spec().revision_id
            || state.revision_digest().as_str() != request.revision.digest()
        {
            return Err(ApplicationError::Conflict(
                "Automation schedule state is bound to a different revision".into(),
            ));
        }
        let reserved = self
            .states
            .reserve(ReserveAutomationScheduleLease {
                key: request.key,
                owner_id: request.owner_id,
                lease_id: request.lease_id,
                reserved_at: request.reserved_at,
                lease_expires_at: request.lease_expires_at,
            })
            .await?;
        let lease_generation = reserved.lease_generation();
        let lease = reserved.lease().ok_or_else(|| {
            ApplicationError::Internal("reserved schedule state lost its lease".into())
        })?;
        let cursor_at = reserved.cursor_at();
        if request.observed_at <= cursor_at {
            lease
                .authorize(request.owner_id, request.lease_id, request.observed_at)
                .map_err(ApplicationError::Conflict)?;
            self.release(&request, lease_generation, request.observed_at)
                .await?;
            return Ok(AutomationScheduleDispatchResult {
                admitted: 0,
                already_admitted: 0,
                skipped: 0,
                evaluated_through: None,
            });
        }

        let evaluation = match AutomationScheduleLeaseEvaluator::evaluate(
            lease,
            AutomationScheduleLeaseEvaluationRequest {
                owner_id: request.owner_id,
                lease_id: request.lease_id,
                calculator: &calculator,
                policy: &definition.policy.misfire,
                cursor: cursor_at,
                observed_at: request.observed_at,
                limit: request.limit,
            },
        ) {
            Ok(evaluation) => evaluation,
            Err(error) => {
                let _ = self
                    .release(&request, lease_generation, request.observed_at)
                    .await;
                return Err(ApplicationError::Invalid(error));
            }
        };

        let mut envelopes = Vec::with_capacity(evaluation.selection.selected.len());
        for scheduled_at in &evaluation.selection.selected {
            let invocation_id =
                schedule_invocation_id(request.revision.spec().revision_id, *scheduled_at);
            let envelope = match AutomationScheduleInvocationFactory::build(
                AutomationScheduleInvocationRequest {
                    revision: &request.revision,
                    invocation_id,
                    scheduled_at: *scheduled_at,
                    // The occurrence is the immutable request instant. This
                    // prevents a later redelivery tick from changing the
                    // exact envelope admitted under the same deduplication key.
                    requested_at: *scheduled_at,
                    input: request.input.clone(),
                    authorization: request.authorization.clone(),
                    correlation_id: Uuid::new_v5(
                        &invocation_id,
                        b"automation.schedule.correlation.v1",
                    ),
                    causation_id: None,
                },
            ) {
                Ok(envelope) => envelope,
                Err(error) => {
                    let _ = self
                        .release(&request, lease_generation, request.observed_at)
                        .await;
                    return Err(ApplicationError::Invalid(error));
                }
            };
            envelopes.push(envelope);
        }

        let mut admitted = 0;
        let mut already_admitted = 0;
        for envelope in envelopes {
            match self.admission.admit(envelope).await {
                Ok(AutomationInvocationAdmissionOutcome::Admitted) => admitted += 1,
                Ok(AutomationInvocationAdmissionOutcome::AlreadyAdmitted) => already_admitted += 1,
                Err(error) => {
                    let _ = self
                        .release(&request, lease_generation, request.observed_at)
                        .await;
                    return Err(error);
                }
            }
        }

        let Some(evaluated_through) = evaluation.evaluated_through else {
            self.release(&request, lease_generation, request.observed_at)
                .await?;
            return Ok(AutomationScheduleDispatchResult {
                admitted,
                already_admitted,
                skipped: evaluation.selection.skipped.len(),
                evaluated_through: None,
            });
        };

        self.states
            .commit_cursor(CommitAutomationScheduleCursor {
                key: request.key,
                owner_id: request.owner_id,
                lease_id: request.lease_id,
                lease_generation,
                evaluated_through,
                committed_at: request.observed_at,
            })
            .await?;
        Ok(AutomationScheduleDispatchResult {
            admitted,
            already_admitted,
            skipped: evaluation.selection.skipped.len(),
            evaluated_through: Some(evaluated_through),
        })
    }

    async fn release(
        &self,
        request: &AutomationScheduleDispatchRequest,
        lease_generation: u64,
        released_at: DateTime<Utc>,
    ) -> ApplicationResult<()> {
        self.states
            .release_lease(ReleaseAutomationScheduleLease {
                key: request.key,
                owner_id: request.owner_id,
                lease_id: request.lease_id,
                lease_generation,
                released_at,
            })
            .await
            .map(|_| ())
            .map_err(Into::into)
    }
}

fn schedule_invocation_id(revision_id: Uuid, scheduled_at: DateTime<Utc>) -> Uuid {
    let identity = format!(
        "automation.schedule.invocation.v1/{}",
        scheduled_at.to_rfc3339_opts(SecondsFormat::Nanos, true)
    );
    Uuid::new_v5(&revision_id, identity.as_bytes())
}

#[async_trait]
pub trait IAutomationScheduleDispatchService: Send + Sync {
    async fn dispatch(
        &self,
        request: AutomationScheduleDispatchRequest,
    ) -> ApplicationResult<AutomationScheduleDispatchResult>;
}

#[async_trait]
impl IAutomationScheduleDispatchService for AutomationScheduleDispatchService {
    async fn dispatch(
        &self,
        request: AutomationScheduleDispatchRequest,
    ) -> ApplicationResult<AutomationScheduleDispatchResult> {
        Self::dispatch(self, request).await
    }
}

#[cfg(test)]
#[path = "schedule_dispatch_tests.rs"]
mod tests;
