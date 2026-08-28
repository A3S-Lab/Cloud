use crate::modules::shared_kernel::domain::{
    canonical_json_bounded, canonical_timestamp, sha256_digest, HumanTaskId, IdempotencyRequest,
    RepositoryError, Sha256Digest, WorkflowDecisionId, WorkflowRunId,
};
use crate::modules::workflow::application::{HumanTaskFormReleaseAuthority, IHumanTaskFormPort};
use crate::modules::workflow::domain::{
    expected_human_task_expiry, AssignmentPolicyRef, ChangeHumanTaskWrite, CreateHumanTaskWrite,
    DecideHumanTaskWrite, FlowResumePayload, HumanTask, HumanTaskCancellationAuthority,
    HumanTaskDeadlineAuthority, HumanTaskDecisionRecord, HumanTaskInteractionSpec,
    HumanTaskParentCancellationEvidence, HumanTaskRecord, HumanTaskStateChanged, HumanTaskStatus,
    IHumanTaskRepository, IWorkflowRunRepository, NewHumanTask, WorkflowDecision,
    WorkflowHumanDecisionHookMetadata, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepKind,
};
use a3s_flow::{FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, HookStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

const FLOW_EVENT_DIGEST_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskCoordinationFailure {
    pub workflow_run_id: WorkflowRunId,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskExpiryFailure {
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanTaskCancellationFailure {
    pub workflow_run_id: WorkflowRunId,
    pub human_task_id: HumanTaskId,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HumanTaskCoordinationReport {
    pub inspected_runs: usize,
    pub observed_hooks: usize,
    pub created_tasks: usize,
    pub replayed_tasks: usize,
    pub activated_tasks: usize,
    pub deferred_tasks: usize,
    pub inspected_cancellations: usize,
    pub cancelled_tasks: usize,
    pub replayed_cancellations: usize,
    pub deferred_cancellations: usize,
    pub contended_cancellations: usize,
    pub inspected_expirations: usize,
    pub expired_tasks: usize,
    pub replayed_expirations: usize,
    pub deferred_expirations: usize,
    pub contended_expirations: usize,
    pub failures: Vec<HumanTaskCoordinationFailure>,
    pub cancellation_failures: Vec<HumanTaskCancellationFailure>,
    pub expiry_failures: Vec<HumanTaskExpiryFailure>,
}

#[derive(Default)]
struct RunCoordination {
    observed_hooks: usize,
    created_tasks: usize,
    replayed_tasks: usize,
    activated_tasks: usize,
    deferred_tasks: usize,
}

enum AutomaticDecisionCoordination {
    Decided,
    Replayed,
    Deferred,
    Contended,
}

pub struct HumanTaskCoordinator {
    workflow_runs: Arc<dyn IWorkflowRunRepository>,
    forms: Arc<dyn IHumanTaskFormPort>,
    human_tasks: Arc<dyn IHumanTaskRepository>,
    engine: FlowEngine,
    interval: Duration,
    batch_size: usize,
}

impl HumanTaskCoordinator {
    pub fn new(
        workflow_runs: Arc<dyn IWorkflowRunRepository>,
        forms: Arc<dyn IHumanTaskFormPort>,
        human_tasks: Arc<dyn IHumanTaskRepository>,
        engine: FlowEngine,
        interval: Duration,
        batch_size: usize,
    ) -> Result<Self, String> {
        if interval.is_zero() || batch_size == 0 {
            return Err(
                "HumanTask coordination requires a positive interval and batch size".into(),
            );
        }
        Ok(Self {
            workflow_runs,
            forms,
            human_tasks,
            engine,
            interval,
            batch_size,
        })
    }

    pub async fn run_once(
        &self,
        limit: usize,
    ) -> Result<HumanTaskCoordinationReport, RepositoryError> {
        self.run_once_at(limit, chrono::Utc::now()).await
    }

    pub async fn run_once_at(
        &self,
        limit: usize,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<HumanTaskCoordinationReport, RepositoryError> {
        let limit = limit.max(1);
        let now = canonical_timestamp(now);
        let runs = self.workflow_runs.pending_reconciliation(limit).await?;
        let mut report = HumanTaskCoordinationReport {
            inspected_runs: runs.len(),
            ..HumanTaskCoordinationReport::default()
        };
        for run in runs {
            match self.coordinate_run(&run).await {
                Ok(progress) => {
                    report.observed_hooks += progress.observed_hooks;
                    report.created_tasks += progress.created_tasks;
                    report.replayed_tasks += progress.replayed_tasks;
                    report.activated_tasks += progress.activated_tasks;
                    report.deferred_tasks += progress.deferred_tasks;
                }
                Err(error) => report.failures.push(HumanTaskCoordinationFailure {
                    workflow_run_id: run.run.id,
                    error,
                }),
            }
        }
        let cancellations = self.human_tasks.pending_parent_cancellations(limit).await?;
        report.inspected_cancellations = cancellations.len();
        for task in cancellations {
            match self.cancel_task(task.clone()).await {
                Ok(AutomaticDecisionCoordination::Decided) => report.cancelled_tasks += 1,
                Ok(AutomaticDecisionCoordination::Replayed) => report.replayed_cancellations += 1,
                Ok(AutomaticDecisionCoordination::Deferred) => report.deferred_cancellations += 1,
                Ok(AutomaticDecisionCoordination::Contended) => report.contended_cancellations += 1,
                Err(error) => report
                    .cancellation_failures
                    .push(HumanTaskCancellationFailure {
                        workflow_run_id: task.task.workflow_run_id,
                        human_task_id: task.task.id,
                        error,
                    }),
            }
        }
        let expirations = self.human_tasks.pending_expirations(now, limit).await?;
        report.inspected_expirations = expirations.len();
        for task in expirations {
            match self.expire_task(task.clone()).await {
                Ok(AutomaticDecisionCoordination::Decided) => report.expired_tasks += 1,
                Ok(AutomaticDecisionCoordination::Replayed) => report.replayed_expirations += 1,
                Ok(AutomaticDecisionCoordination::Deferred) => report.deferred_expirations += 1,
                Ok(AutomaticDecisionCoordination::Contended) => report.contended_expirations += 1,
                Err(error) => report.expiry_failures.push(HumanTaskExpiryFailure {
                    workflow_run_id: task.task.workflow_run_id,
                    human_task_id: task.task.id,
                    error,
                }),
            }
        }
        Ok(report)
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    match self.run_once(self.batch_size).await {
                        Ok(report) => {
                            for failure in report.failures {
                                tracing::warn!(
                                    workflow_run_id = %failure.workflow_run_id,
                                    error = %failure.error,
                                    "HumanTask Flow-hook coordination failed"
                                );
                            }
                            for failure in report.expiry_failures {
                                tracing::warn!(
                                    workflow_run_id = %failure.workflow_run_id,
                                    human_task_id = %failure.human_task_id,
                                    error = %failure.error,
                                    "HumanTask deadline coordination failed"
                                );
                            }
                            for failure in report.cancellation_failures {
                                tracing::warn!(
                                    workflow_run_id = %failure.workflow_run_id,
                                    human_task_id = %failure.human_task_id,
                                    error = %failure.error,
                                    "HumanTask parent cancellation coordination failed"
                                );
                            }
                        }
                        Err(error) => tracing::error!(
                            error = %error,
                            "HumanTask coordination scan failed"
                        ),
                    }
                }
            }
        }
    }

    async fn coordinate_run(&self, record: &WorkflowRunRecord) -> Result<RunCoordination, String> {
        record.validate()?;
        let history = match self.engine.history(&record.run.flow_run_id).await {
            Ok(history) => history,
            Err(FlowError::RunNotFound(_)) => return Ok(RunCoordination::default()),
            Err(error) => return Err(format!("could not read WorkflowRun Flow history: {error}")),
        };
        let snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
            Ok(snapshot) => snapshot,
            Err(FlowError::RunNotFound(_)) => return Ok(RunCoordination::default()),
            Err(error) => {
                return Err(format!(
                    "could not project WorkflowRun Flow history: {error}"
                ))
            }
        };
        if snapshot.run_id != record.run.flow_run_id
            || history
                .iter()
                .any(|event| event.run_id != record.run.flow_run_id)
        {
            return Err("WorkflowRun Flow history identity drifted".into());
        }

        let steps = record.run.execution_input.resolved_steps()?;
        let human_steps = steps
            .iter()
            .filter(|step| step.plan.kind == WorkflowStepKind::HumanDecision)
            .map(|step| (step.plan.id.as_str(), step))
            .collect::<BTreeMap<_, _>>();
        let expected_hook_ids = human_steps
            .values()
            .map(|step| {
                WorkflowHumanDecisionHookMetadata::from_run_step(&record.run.execution_input, step)
                    .map(|metadata| metadata.flow_hook_id())
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        for envelope in &history {
            let FlowEvent::HookCreated { hook_id, .. } = &envelope.event else {
                continue;
            };
            if hook_id.starts_with("workflow-human:") && !expected_hook_ids.contains(hook_id) {
                return Err(format!(
                    "WorkflowRun Flow history contains unexpected human hook {hook_id:?}"
                ));
            }
        }

        let mut progress = RunCoordination::default();
        for step in human_steps.values() {
            let expected = WorkflowHumanDecisionHookMetadata::from_run_step(
                &record.run.execution_input,
                step,
            )?;
            let hook_id = expected.flow_hook_id();
            let matching = history
                .iter()
                .filter(|envelope| {
                    matches!(
                        &envelope.event,
                        FlowEvent::HookCreated { hook_id: observed, .. } if observed == &hook_id
                    )
                })
                .collect::<Vec<_>>();
            if matching.is_empty() {
                continue;
            }
            if matching.len() != 1 {
                return Err(format!(
                    "WorkflowRun Flow history contains duplicate HookCreated events for {hook_id:?}"
                ));
            }
            progress.observed_hooks += 1;
            let envelope = matching[0];
            let FlowEvent::HookCreated {
                token, metadata, ..
            } = &envelope.event
            else {
                unreachable!("matching history was filtered to HookCreated")
            };
            let observed: WorkflowHumanDecisionHookMetadata =
                serde_json::from_value(metadata.clone())
                    .map_err(|error| format!("human hook metadata is not closed: {error}"))?;
            observed.validate()?;
            if observed != expected
                || token != &expected.flow_hook_token()
                || metadata
                    != &serde_json::to_value(&expected).map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "WorkflowRun human hook {hook_id:?} authority drifted"
                ));
            }

            let release_ref = self
                .forms
                .resolve_interaction_release(&HumanTaskFormReleaseAuthority {
                    organization_id: expected.organization_id,
                    project_id: expected.project_id,
                    form_id: expected.form_id,
                    form_release_id: expected.form_release_id,
                    form_release_digest: expected.form_release_digest.clone(),
                })
                .await
                .map_err(|error| error.to_string())?;

            let created_at = canonical_timestamp(envelope.timestamp);
            let expires_at = expected_human_task_expiry(record, step, created_at)?;
            let task_id = deterministic_human_task_id(record.run.id, &expected, envelope.event_id);
            let task = HumanTask::create(NewHumanTask {
                organization_id: expected.organization_id,
                project_id: expected.project_id,
                id: task_id,
                workflow_run_id: expected.workflow_run_id,
                step_id: expected.step_id.clone(),
                step_attempt: expected.step_attempt,
                form_release: release_ref,
                assignment_policy: AssignmentPolicyRef::workflow_organization_member_exclusive()?,
                flow_run_id: record.run.flow_run_id.clone(),
                flow_hook_id: hook_id.clone(),
                due_at: None,
                expires_at: Some(expires_at),
                created_at,
            })?;
            let requested = HumanTaskRecord::create(
                task,
                HumanTaskInteractionSpec::approval(
                    step.configuration
                        .message
                        .clone()
                        .ok_or_else(|| "human-decision message disappeared".to_owned())?,
                    step.configuration.details.clone(),
                    None,
                )?,
                envelope.sequence,
                envelope.event_id,
            )?;
            let hook_digest = flow_event_digest(envelope)?;
            let created_event =
                HumanTaskStateChanged::envelope(&requested, Some(envelope.event_id)).map_err(
                    |error| format!("could not encode HumanTask creation event: {error}"),
                )?;
            let stored = self
                .human_tasks
                .create_from_hook(CreateHumanTaskWrite {
                    record: requested,
                    hook_event_digest: hook_digest,
                    hook_observed_at: created_at,
                    event: created_event,
                    request_id: Uuid::new_v5(&task_id.as_uuid(), b"create-request"),
                })
                .await
                .map_err(|error| format!("could not persist HumanTask from Flow hook: {error}"))?;
            if stored.replayed {
                progress.replayed_tasks += 1;
            } else {
                progress.created_tasks += 1;
            }

            let Some(active_hook) = snapshot.hooks.get(&hook_id) else {
                return Err(format!("WorkflowRun snapshot lost human hook {hook_id:?}"));
            };
            if active_hook.token != expected.flow_hook_token()
                || active_hook.metadata
                    != serde_json::to_value(&expected).map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "WorkflowRun snapshot human hook {hook_id:?} authority drifted"
                ));
            }
            if active_hook.status != HookStatus::Active {
                progress.deferred_tasks += 1;
                continue;
            }
            if stored.value.task.status != HumanTaskStatus::PendingActivation {
                continue;
            }
            let mut ready = stored.value;
            ready.activate(1, created_at)?;
            let activation_event = HumanTaskStateChanged::envelope(&ready, Some(envelope.event_id))
                .map_err(|error| format!("could not encode HumanTask activation event: {error}"))?;
            let activation_content = format!(
                "{}:{}:{}:{}",
                ready.task.id,
                ready.task.workflow_run_id,
                ready.task.step_id,
                ready.task.aggregate_version
            );
            self.human_tasks
                .change_task(ChangeHumanTaskWrite {
                    record: ready,
                    expected_version: 1,
                    event: activation_event,
                    actor_principal_id: record.run.requested_by,
                    request_id: Uuid::new_v5(&task_id.as_uuid(), b"activate-request"),
                    idempotency: IdempotencyRequest::new(
                        "workflow-human-task-activation",
                        task_id.to_string(),
                        activation_content.as_bytes(),
                    )?,
                })
                .await
                .map_err(|error| format!("could not activate HumanTask: {error}"))?;
            progress.activated_tasks += 1;
        }
        Ok(progress)
    }

    async fn cancel_task(
        &self,
        mut task: HumanTaskRecord,
    ) -> Result<AutomaticDecisionCoordination, String> {
        let run = self
            .workflow_runs
            .find(task.task.organization_id, task.task.workflow_run_id)
            .await
            .map_err(|error| format!("could not load HumanTask WorkflowRun: {error}"))?
            .ok_or_else(|| "HumanTask WorkflowRun does not exist".to_owned())?;
        match run.run.status {
            WorkflowRunStatus::Cancelling => return Ok(AutomaticDecisionCoordination::Deferred),
            WorkflowRunStatus::Cancelled => {}
            _ => return Ok(AutomaticDecisionCoordination::Contended),
        }
        let history = match self.engine.history(&run.run.flow_run_id).await {
            Ok(history) => history,
            Err(FlowError::RunNotFound(_)) => return Ok(AutomaticDecisionCoordination::Deferred),
            Err(error) => {
                return Err(format!(
                    "could not read cancelled WorkflowRun Flow history: {error}"
                ))
            }
        };
        if history
            .iter()
            .any(|envelope| envelope.run_id != run.run.flow_run_id)
        {
            return Err("cancelled WorkflowRun Flow history identity drifted".into());
        }
        let cancellation = exact_parent_cancellation_evidence(&history)?;
        let authority = HumanTaskCancellationAuthority::derive(&run, &task, &cancellation)?;
        let expected_version = task.task.aggregate_version;
        let decision_id = deterministic_cancellation_decision_id(&task, &cancellation);
        let decision = WorkflowDecision::cancel(
            decision_id,
            &task.task,
            authority.decided_by,
            authority.authorization_decision,
            authority.decided_at,
        )?;
        task.cancel(expected_version, &decision)?;
        self.persist_automatic_decision(
            task,
            expected_version,
            decision,
            Some(cancellation.cancelled_event_id),
            "workflow-human-task-parent-cancellation",
            b"cancel-request",
            "cancellation",
        )
        .await
    }

    async fn expire_task(
        &self,
        mut task: HumanTaskRecord,
    ) -> Result<AutomaticDecisionCoordination, String> {
        let run = self
            .workflow_runs
            .find(task.task.organization_id, task.task.workflow_run_id)
            .await
            .map_err(|error| format!("could not load HumanTask WorkflowRun: {error}"))?
            .ok_or_else(|| "HumanTask WorkflowRun does not exist".to_owned())?;
        if matches!(
            run.run.status,
            WorkflowRunStatus::Cancelling | WorkflowRunStatus::Cancelled
        ) {
            return Ok(AutomaticDecisionCoordination::Deferred);
        }
        let authority = HumanTaskDeadlineAuthority::derive(&run, &task)?;
        let expected_version = task.task.aggregate_version;
        let decision_id = deterministic_expiry_decision_id(&task);
        let decision = WorkflowDecision::expire(
            decision_id,
            &task.task,
            authority.decided_by,
            authority.authorization_decision,
            authority.decided_at,
        )?;
        task.expire(expected_version, &decision)?;
        self.persist_automatic_decision(
            task,
            expected_version,
            decision,
            Some(decision_id.as_uuid()),
            "workflow-human-task-expiry",
            b"expire-request",
            "expiry",
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn persist_automatic_decision(
        &self,
        task: HumanTaskRecord,
        expected_version: u64,
        decision: WorkflowDecision,
        causation_id: Option<Uuid>,
        idempotency_scope: &'static str,
        request_identity: &'static [u8],
        action: &'static str,
    ) -> Result<AutomaticDecisionCoordination, String> {
        let resume_payload = FlowResumePayload::from_decision(&decision)?;
        let event = HumanTaskStateChanged::envelope(&task, causation_id)
            .map_err(|error| format!("could not encode HumanTask {action} event: {error}"))?;
        let idempotency = IdempotencyRequest::new(
            idempotency_scope,
            format!("{}:v{expected_version}", task.task.id),
            decision.digest.as_str().as_bytes(),
        )?;
        let decision_id = decision.id;
        let actor_principal_id = decision.decided_by;
        let write = DecideHumanTaskWrite {
            record: HumanTaskDecisionRecord {
                task,
                submission: None,
                decision,
                resume_payload,
                resume_receipt: None,
            },
            expected_version,
            event,
            actor_principal_id,
            request_id: Uuid::new_v5(&decision_id.as_uuid(), request_identity),
            idempotency,
        };
        let organization_id = write.record.task.task.organization_id;
        let human_task_id = write.record.task.task.id;
        match self.human_tasks.decide_task(write).await {
            Ok(stored) if stored.replayed => Ok(AutomaticDecisionCoordination::Replayed),
            Ok(_) => Ok(AutomaticDecisionCoordination::Decided),
            Err(RepositoryError::Conflict(error)) => {
                let current = self
                    .human_tasks
                    .find_task(organization_id, human_task_id)
                    .await
                    .map_err(|find_error| {
                        format!(
                            "HumanTask {action} conflicted ({error}) and recovery read failed: {find_error}"
                        )
                    })?;
                if current.is_some_and(|record| {
                    record.task.aggregate_version != expected_version
                        || record.task.status.is_terminal()
                }) {
                    Ok(AutomaticDecisionCoordination::Contended)
                } else {
                    Err(format!(
                        "HumanTask {action} conflicted without a concurrent state change: {error}"
                    ))
                }
            }
            Err(error) => Err(format!("could not persist HumanTask {action}: {error}")),
        }
    }
}

fn deterministic_human_task_id(
    workflow_run_id: WorkflowRunId,
    metadata: &WorkflowHumanDecisionHookMetadata,
    hook_event_id: Uuid,
) -> HumanTaskId {
    let identity = format!(
        "human-task:{}:{}:{hook_event_id}",
        metadata.step_id, metadata.step_attempt
    );
    HumanTaskId::from_uuid(Uuid::new_v5(
        &workflow_run_id.as_uuid(),
        identity.as_bytes(),
    ))
}

fn deterministic_expiry_decision_id(task: &HumanTaskRecord) -> WorkflowDecisionId {
    let identity = format!(
        "expire:v{}:{}",
        task.task.aggregate_version,
        task.task
            .expires_at
            .map(|value| value.timestamp_micros())
            .unwrap_or_default()
    );
    WorkflowDecisionId::from_uuid(Uuid::new_v5(&task.task.id.as_uuid(), identity.as_bytes()))
}

fn deterministic_cancellation_decision_id(
    task: &HumanTaskRecord,
    cancellation: &HumanTaskParentCancellationEvidence,
) -> WorkflowDecisionId {
    let identity = format!(
        "parent-cancel:v{}:{}:{}",
        task.task.aggregate_version,
        cancellation.cancelled_sequence,
        cancellation.cancelled_event_id
    );
    WorkflowDecisionId::from_uuid(Uuid::new_v5(&task.task.id.as_uuid(), identity.as_bytes()))
}

fn exact_parent_cancellation_evidence(
    history: &[FlowEventEnvelope],
) -> Result<HumanTaskParentCancellationEvidence, String> {
    let requests = history
        .iter()
        .filter(|envelope| matches!(envelope.event, FlowEvent::RunCancellationRequested { .. }))
        .collect::<Vec<_>>();
    let cancellations = history
        .iter()
        .filter(|envelope| matches!(envelope.event, FlowEvent::RunCancelled { .. }))
        .collect::<Vec<_>>();
    let ([request], [cancelled]) = (requests.as_slice(), cancellations.as_slice()) else {
        return Err(format!(
            "cancelled WorkflowRun requires exactly one RunCancellationRequested and one RunCancelled event; observed {} and {}",
            requests.len(),
            cancellations.len()
        ));
    };
    let FlowEvent::RunCancellationRequested {
        request: cancellation_request,
    } = &request.event
    else {
        unreachable!("request history was filtered to RunCancellationRequested")
    };
    let FlowEvent::RunCancelled { reason } = &cancelled.event else {
        unreachable!("cancellation history was filtered to RunCancelled")
    };
    Ok(HumanTaskParentCancellationEvidence {
        flow_run_id: cancelled.run_id.clone(),
        request_sequence: request.sequence,
        request_event_id: request.event_id,
        request_event_at: canonical_timestamp(request.timestamp),
        request_reason: cancellation_request.reason.clone(),
        cancelled_sequence: cancelled.sequence,
        cancelled_event_id: cancelled.event_id,
        cancelled_event_at: canonical_timestamp(cancelled.timestamp),
        cancelled_reason: reason.clone(),
    })
}

fn flow_event_digest(envelope: &FlowEventEnvelope) -> Result<Sha256Digest, String> {
    let canonical = canonical_json_bounded(
        envelope,
        FLOW_EVENT_DIGEST_MAX_BYTES,
        "HumanTask Flow HookCreated evidence",
    )?;
    Sha256Digest::parse(sha256_digest(&canonical))
}
