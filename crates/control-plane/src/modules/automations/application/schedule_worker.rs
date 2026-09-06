use crate::modules::automations::application::schedule_dispatch::{
    AutomationScheduleDispatchRequest, AutomationScheduleDispatchResult,
    IAutomationScheduleDispatchService,
};
use crate::modules::automations::domain::{
    AutomationScheduleStateKey, AUTOMATION_SCHEDULE_MAX_LEASE_MS,
    AUTOMATION_SCHEDULE_MAX_OCCURRENCES,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_cloud_contracts::{
    AutomationInvocationAuthorizationV1, AutomationInvocationInputV1, AutomationRevisionV1,
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

/// One schedule revision selected by the owner of Automation definitions.
///
/// The worker never discovers or persists these values. The provider must
/// return the exact immutable revision, schedule-state key, input, and
/// authorization snapshot that can be safely handed to the dispatch service.
#[derive(Debug, Clone)]
pub struct AutomationScheduleCandidate {
    pub key: AutomationScheduleStateKey,
    pub revision: AutomationRevisionV1,
    pub input: AutomationInvocationInputV1,
    pub authorization: AutomationInvocationAuthorizationV1,
    pub limit: usize,
}

/// Owner port for bounded schedule candidate discovery.
///
/// Implementations retain definition/revision persistence and authorization
/// ownership. A provider failure is returned to the worker so the next tick
/// can retry without advancing any cursor.
#[async_trait]
pub trait IAutomationScheduleCandidateProvider: Send + Sync {
    async fn candidates(
        &self,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<Vec<AutomationScheduleCandidate>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomationScheduleWorkerConfig {
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub max_candidates: usize,
}

impl AutomationScheduleWorkerConfig {
    pub fn new(
        poll_interval: Duration,
        lease_duration: Duration,
        max_candidates: usize,
    ) -> Result<Self, String> {
        if poll_interval.is_zero() || poll_interval > Duration::from_secs(24 * 60 * 60) {
            return Err("Automation schedule worker poll interval is outside its bound".into());
        }
        if lease_duration.is_zero()
            || lease_duration > Duration::from_millis(AUTOMATION_SCHEDULE_MAX_LEASE_MS as u64)
        {
            return Err("Automation schedule worker lease duration is outside its bound".into());
        }
        if max_candidates == 0 || max_candidates > 10_000 {
            return Err("Automation schedule worker candidate bound is outside its limit".into());
        }
        Ok(Self {
            poll_interval,
            lease_duration,
            max_candidates,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationScheduleWorkerReport {
    pub candidates: usize,
    pub dispatched: usize,
    pub already_admitted: usize,
    pub failed: Vec<String>,
}

impl AutomationScheduleWorkerReport {
    fn empty() -> Self {
        Self {
            candidates: 0,
            dispatched: 0,
            already_admitted: 0,
            failed: Vec::new(),
        }
    }
}

/// Timer owner for the bounded schedule-dispatch service.
///
/// This component owns only the process-local polling loop and a stable worker
/// owner identity. Durable cursor/lease state remains in the injected schedule
/// repository, while revision discovery and target handoff remain external
/// ports. A failed candidate is recorded and does not prevent another bounded
/// candidate from being attempted; the failed cursor remains recoverable on a
/// later tick because dispatch releases its lease without cursor progress.
#[derive(Clone)]
pub struct AutomationScheduleWorker {
    owner_id: Uuid,
    candidates: Arc<dyn IAutomationScheduleCandidateProvider>,
    dispatch: Arc<dyn IAutomationScheduleDispatchService>,
    config: AutomationScheduleWorkerConfig,
}

impl AutomationScheduleWorker {
    pub fn new(
        owner_id: Uuid,
        candidates: Arc<dyn IAutomationScheduleCandidateProvider>,
        dispatch: Arc<dyn IAutomationScheduleDispatchService>,
        config: AutomationScheduleWorkerConfig,
    ) -> Result<Self, String> {
        if owner_id.is_nil() {
            return Err("Automation schedule worker owner identity is invalid".into());
        }
        Ok(Self {
            owner_id,
            candidates,
            dispatch,
            config,
        })
    }

    pub async fn tick_at(
        &self,
        observed_at: DateTime<Utc>,
    ) -> ApplicationResult<AutomationScheduleWorkerReport> {
        let mut candidates = self.candidates.candidates(observed_at).await?;
        if candidates.len() > self.config.max_candidates {
            return Err(ApplicationError::Invalid(
                "Automation schedule candidate result exceeds its bound".into(),
            ));
        }
        candidates.sort_by(|left, right| {
            left.key
                .cmp(&right.key)
                .then_with(|| left.revision.digest().cmp(right.revision.digest()))
        });

        let mut report = AutomationScheduleWorkerReport {
            candidates: candidates.len(),
            ..AutomationScheduleWorkerReport::empty()
        };
        let mut seen = BTreeSet::new();
        let lease_duration =
            ChronoDuration::from_std(self.config.lease_duration).map_err(|_| {
                ApplicationError::Invalid("Automation schedule lease duration overflowed".into())
            })?;
        let lease_expires_at = observed_at
            .checked_add_signed(lease_duration)
            .ok_or_else(|| {
                ApplicationError::Invalid("Automation schedule lease timestamp overflowed".into())
            })?;

        for candidate in candidates {
            let identity = (candidate.key, candidate.revision.digest().to_owned());
            if !seen.insert(identity) {
                report
                    .failed
                    .push("duplicate Automation schedule candidate".into());
                continue;
            }
            if candidate.limit == 0 || candidate.limit > AUTOMATION_SCHEDULE_MAX_OCCURRENCES {
                report.failed.push(
                    "Automation schedule candidate occurrence limit is outside its bound".into(),
                );
                continue;
            }
            let request = AutomationScheduleDispatchRequest {
                key: candidate.key,
                revision: candidate.revision,
                owner_id: self.owner_id,
                lease_id: Uuid::now_v7(),
                reserved_at: observed_at,
                lease_expires_at,
                observed_at,
                limit: candidate.limit,
                input: candidate.input,
                authorization: candidate.authorization,
            };
            match self.dispatch.dispatch(request).await {
                Ok(AutomationScheduleDispatchResult {
                    admitted,
                    already_admitted,
                    ..
                }) => {
                    report.dispatched += admitted;
                    report.already_admitted += already_admitted;
                }
                Err(error) => report.failed.push(error.to_string()),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        if *shutdown.borrow() {
            return;
        }
        let mut ticker = tokio::time::interval(self.config.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match self.tick_at(Utc::now()).await {
                        Ok(report) if !report.failed.is_empty() => {
                            tracing::warn!(failed = report.failed.len(), "Automation schedule tick had candidate failures");
                        }
                        Ok(_) => {}
                        Err(error) => {
                            tracing::error!(error = %error, "Automation schedule candidate discovery failed");
                        }
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::automations::application::AutomationScheduleDispatchRequest;
    use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
    use a3s_cloud_contracts::{
        AutomationDefinitionV1, AutomationInvocationInputV1, AutomationRevisionV1,
    };
    use async_trait::async_trait;
    use chrono::TimeZone;
    use serde_json::json;
    use std::sync::Mutex;

    const SCHEDULE_DEFINITION: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/aut0.1/automation-definition-schedule.acl"
    ));

    struct CandidateProvider {
        candidates: Mutex<Vec<AutomationScheduleCandidate>>,
    }

    #[async_trait]
    impl IAutomationScheduleCandidateProvider for CandidateProvider {
        async fn candidates(
            &self,
            _observed_at: DateTime<Utc>,
        ) -> ApplicationResult<Vec<AutomationScheduleCandidate>> {
            Ok(self.candidates.lock().expect("candidate lock").clone())
        }
    }

    #[derive(Default)]
    struct RecordingDispatch {
        requests: Mutex<Vec<AutomationScheduleDispatchRequest>>,
        failures: Mutex<BTreeSet<Uuid>>,
    }

    #[async_trait]
    impl IAutomationScheduleDispatchService for RecordingDispatch {
        async fn dispatch(
            &self,
            request: AutomationScheduleDispatchRequest,
        ) -> ApplicationResult<AutomationScheduleDispatchResult> {
            let automation_id = request.key.automation_id;
            self.requests.lock().expect("request lock").push(request);
            if self
                .failures
                .lock()
                .expect("failure lock")
                .contains(&automation_id)
            {
                return Err(ApplicationError::Unavailable(
                    "fixture dispatch failure".into(),
                ));
            }
            Ok(AutomationScheduleDispatchResult {
                admitted: 1,
                already_admitted: 0,
                skipped: 0,
                evaluated_through: None,
            })
        }
    }

    fn candidate(
        automation_id: Uuid,
        revision: &AutomationRevisionV1,
    ) -> AutomationScheduleCandidate {
        let definition = &revision.spec().definition;
        AutomationScheduleCandidate {
            key: AutomationScheduleStateKey::new(
                OrganizationId::from_uuid(definition.organization_id),
                ProjectId::from_uuid(definition.project_id),
                EnvironmentId::from_uuid(definition.environment_id),
                automation_id,
            ),
            revision: revision.clone(),
            input: AutomationInvocationInputV1::inline_json(json!({"source": "worker-test"}))
                .expect("input"),
            authorization: AutomationInvocationAuthorizationV1 {
                policy_digest: definition.authorization.policy_digest.clone(),
                grant_snapshot_digest:
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                principal_id: None,
            },
            limit: 1,
        }
    }

    fn revision() -> AutomationRevisionV1 {
        let definition =
            AutomationDefinitionV1::parse_acl(SCHEDULE_DEFINITION).expect("definition");
        AutomationRevisionV1::from_definition(
            Uuid::from_u128(0x018f0000000070008000000000000501),
            1,
            None,
            definition.spec().clone(),
        )
        .expect("revision")
    }

    #[test]
    fn worker_config_rejects_unbounded_values() {
        assert!(
            AutomationScheduleWorkerConfig::new(Duration::ZERO, Duration::from_secs(1), 1,)
                .is_err()
        );
        assert!(AutomationScheduleWorkerConfig::new(
            Duration::from_secs(1),
            Duration::from_millis((AUTOMATION_SCHEDULE_MAX_LEASE_MS + 1) as u64),
            1,
        )
        .is_err());
        assert!(AutomationScheduleWorkerConfig::new(
            Duration::from_secs(1),
            Duration::from_secs(1),
            0,
        )
        .is_err());
    }

    #[tokio::test]
    async fn tick_is_deterministic_bounded_and_continues_after_one_failure() {
        let revision = revision();
        let first_id = Uuid::from_u128(0x018f0000000070008000000000000502);
        let second_id = Uuid::from_u128(0x018f0000000070008000000000000503);
        let duplicate = candidate(first_id, &revision);
        let mut failing = candidate(second_id, &revision);
        failing.revision = revision.clone();
        let provider = Arc::new(CandidateProvider {
            candidates: Mutex::new(vec![failing, duplicate.clone(), duplicate]),
        });
        let dispatch = Arc::new(RecordingDispatch::default());
        dispatch
            .failures
            .lock()
            .expect("failure lock")
            .insert(second_id);
        let config =
            AutomationScheduleWorkerConfig::new(Duration::from_secs(1), Duration::from_secs(30), 3)
                .expect("config");
        let worker = AutomationScheduleWorker::new(
            Uuid::from_u128(0x018f0000000070008000000000000504),
            provider,
            dispatch.clone(),
            config,
        )
        .expect("worker");

        let report = worker
            .tick_at(Utc.timestamp_opt(1_789_000_000, 0).single().expect("time"))
            .await
            .expect("tick");
        assert_eq!(report.candidates, 3);
        assert_eq!(report.dispatched, 1);
        assert_eq!(report.already_admitted, 0);
        assert_eq!(report.failed.len(), 2);
        assert_eq!(dispatch.requests.lock().expect("request lock").len(), 2);
        assert!(
            dispatch
                .requests
                .lock()
                .expect("request lock")
                .iter()
                .all(|request| request.owner_id
                    == Uuid::from_u128(0x018f0000000070008000000000000504))
        );
    }

    #[tokio::test]
    async fn worker_stops_without_a_tick_when_shutdown_is_already_requested() {
        let provider = Arc::new(CandidateProvider {
            candidates: Mutex::new(Vec::new()),
        });
        let dispatch = Arc::new(RecordingDispatch::default());
        let config = AutomationScheduleWorkerConfig::new(
            Duration::from_millis(1),
            Duration::from_secs(1),
            1,
        )
        .expect("config");
        let worker = AutomationScheduleWorker::new(Uuid::from_u128(6), provider, dispatch, config)
            .expect("worker");
        let (sender, receiver) = watch::channel(true);
        drop(sender);
        worker.run(receiver).await;
    }
}
