use super::workflow::inactive_step_ids;
use super::{WorkflowLocalStepResult, WorkflowRunInput};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::domain::{
    flow_step_id, IWorkflowRunHistoryReader, WorkflowRunFlowState, WorkflowRunHistoryEvent,
    WorkflowRunHistoryPage, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepFlowState,
    WorkflowStepProjectionStatus, WORKFLOW_RUN_FLOW_NAME, WORKFLOW_RUN_FLOW_VERSION,
    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, RuntimeKind, StepStatus,
    WorkflowRunSnapshot, WorkflowRunStatus as FlowRunStatus, WorkflowTerminalOutcome,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::BTreeMap;

pub fn project_workflow_run_record(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Result<Option<WorkflowRunRecord>, String> {
    verify_flow_authority(record, snapshot, history)?;
    let build_id = snapshot
        .spec
        .runtime_build_id
        .as_ref()
        .ok_or_else(|| "WorkflowRun Flow history is not pinned to a runtime build".to_owned())?
        .as_str()
        .to_owned();
    let started_at = event_time(history, |event| matches!(event, FlowEvent::RunStarted));
    let finished_at = snapshot
        .status
        .is_terminal()
        .then(|| history.last().map(|event| event.timestamp))
        .flatten();
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .ok_or_else(|| "WorkflowRun Flow history is empty".to_owned())?;
    let (status, output, error) = project_run_state(snapshot)?;
    let mut projected = record.clone();
    let expected_version = projected.run.aggregate_version;
    let changed = projected.run.project_flow(WorkflowRunFlowState {
        status,
        flow_runtime_build_id: build_id,
        last_flow_sequence: snapshot.last_sequence,
        output,
        error,
        started_at,
        finished_at,
        observed_at,
    })?;
    if !changed {
        return Ok(None);
    }

    let completed = snapshot
        .steps
        .values()
        .filter_map(|step| {
            step.output
                .as_ref()
                .map(|output| serde_json::from_value::<WorkflowLocalStepResult>(output.clone()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("WorkflowRun Flow step result is invalid: {error}"))?
        .into_iter()
        .map(|result| (result.step_id.clone(), result))
        .collect::<BTreeMap<_, _>>();
    let inactive = inactive_step_ids(&record.run.execution_input, &completed)?;
    let resolved_steps = record.run.execution_input.resolved_steps()?;

    for projection in &mut projected.steps {
        let resolved = resolved_steps
            .iter()
            .find(|step| step.plan.id == projection.step_id)
            .ok_or_else(|| format!("WorkflowRun lost resolved step {:?}", projection.step_id))?;
        let durable_step_id = flow_step_id(&projection.step_id);
        let flow_step = snapshot.steps.get(&durable_step_id);
        let (step_status, attempt, result, selected_handle, step_error, sequence, at) =
            if let Some(flow_step) = flow_step {
                let sequence = last_step_sequence(history, &durable_step_id)
                    .ok_or_else(|| format!("Flow step {durable_step_id:?} has no history"))?;
                let at = history
                    .iter()
                    .find(|event| event.sequence == sequence)
                    .map(|event| event.timestamp)
                    .ok_or_else(|| format!("Flow step {durable_step_id:?} time is missing"))?;
                let status = match flow_step.status {
                    StepStatus::Pending => WorkflowStepProjectionStatus::Pending,
                    StepStatus::Running => WorkflowStepProjectionStatus::Running,
                    StepStatus::Completed => WorkflowStepProjectionStatus::Completed,
                    StepStatus::Failed => WorkflowStepProjectionStatus::Failed,
                    StepStatus::Cancelled => WorkflowStepProjectionStatus::Cancelled,
                };
                let result = flow_step
                    .output
                    .as_ref()
                    .map(|value| {
                        let result =
                            serde_json::from_value::<WorkflowLocalStepResult>(value.clone())
                                .map_err(|error| {
                                    format!(
                                        "Flow step {durable_step_id:?} result is invalid: {error}"
                                    )
                                })?;
                        result.validate(resolved)?;
                        Ok::<WorkflowLocalStepResult, String>(result)
                    })
                    .transpose()?;
                let selected_handle = result
                    .as_ref()
                    .and_then(|result| result.selected_handle.clone());
                let output = result.map(|result| result.output);
                (
                    status,
                    flow_step.attempt,
                    output,
                    selected_handle,
                    flow_step.error.clone(),
                    sequence,
                    at,
                )
            } else if inactive.contains(&projection.step_id) {
                (
                    WorkflowStepProjectionStatus::Skipped,
                    0,
                    None,
                    None,
                    None,
                    snapshot.last_sequence,
                    observed_at,
                )
            } else if status.is_terminal() {
                (
                    WorkflowStepProjectionStatus::Cancelled,
                    0,
                    None,
                    None,
                    None,
                    snapshot.last_sequence,
                    observed_at,
                )
            } else {
                continue;
            };
        let desired = WorkflowStepFlowState {
            status: step_status,
            attempt_generation: attempt,
            selected_handle,
            result,
            error: step_error,
            last_flow_sequence: sequence,
            observed_at: at,
        };
        if projection.status.is_terminal()
            && projection.status == desired.status
            && projection.attempt_generation == desired.attempt_generation
            && projection.selected_handle == desired.selected_handle
            && projection.result == desired.result
            && projection.error == desired.error
        {
            continue;
        }
        projection.project_flow(desired)?;
    }
    if projected.run.aggregate_version != expected_version + 1 {
        return Err("WorkflowRun projection did not advance exactly one aggregate version".into());
    }
    projected.validate()?;
    Ok(Some(projected))
}

fn verify_flow_authority(
    record: &WorkflowRunRecord,
    snapshot: &WorkflowRunSnapshot,
    history: &[FlowEventEnvelope],
) -> Result<(), String> {
    if snapshot.run_id != record.run.flow_run_id
        || snapshot.spec.name != WORKFLOW_RUN_FLOW_NAME
        || snapshot.spec.version != WORKFLOW_RUN_FLOW_VERSION
        || snapshot.spec.runtime.kind != RuntimeKind::RustEmbedded
        || snapshot.spec.runtime.entrypoint != "a3s-cloud"
        || snapshot.spec.runtime.export_name != "main"
        || snapshot.last_sequence == 0
        || history.last().map(|event| event.sequence) != Some(snapshot.last_sequence)
        || history.iter().any(|event| event.run_id != snapshot.run_id)
    {
        return Err("WorkflowRun correlated Flow identity or history sequence drifted".into());
    }
    let expected_input = serde_json::to_value(&record.run.execution_input)
        .map_err(|error| format!("could not encode WorkflowRun input: {error}"))?;
    if snapshot.input != expected_input {
        return Err("WorkflowRun correlated Flow input drifted".into());
    }
    Ok(())
}

fn project_run_state(
    snapshot: &WorkflowRunSnapshot,
) -> Result<(WorkflowRunStatus, Option<serde_json::Value>, Option<String>), String> {
    match snapshot.status {
        FlowRunStatus::Pending => Ok((WorkflowRunStatus::Pending, None, None)),
        FlowRunStatus::Running => Ok((WorkflowRunStatus::Running, None, None)),
        FlowRunStatus::Suspended => Ok((WorkflowRunStatus::Waiting, None, None)),
        FlowRunStatus::Cancelling => Ok((WorkflowRunStatus::Cancelling, None, None)),
        FlowRunStatus::Completed => Ok((
            WorkflowRunStatus::Completed,
            Some(
                snapshot
                    .output
                    .clone()
                    .ok_or_else(|| "completed Flow run is missing output".to_owned())?,
            ),
            None,
        )),
        FlowRunStatus::Cancelled => Ok((WorkflowRunStatus::Cancelled, None, None)),
        FlowRunStatus::Failed => match &snapshot.terminal_outcome {
            Some(WorkflowTerminalOutcome::TimedOut { reason, deadline }) => Ok((
                WorkflowRunStatus::TimedOut,
                None,
                Some(
                    reason
                        .clone()
                        .unwrap_or_else(|| format!("WorkflowRun timed out at {deadline}")),
                ),
            )),
            _ => Ok((
                WorkflowRunStatus::Failed,
                None,
                Some(
                    snapshot
                        .error
                        .clone()
                        .unwrap_or_else(|| "WorkflowRun failed without a Flow error".into()),
                ),
            )),
        },
    }
}

fn event_time(
    history: &[FlowEventEnvelope],
    predicate: impl Fn(&FlowEvent) -> bool,
) -> Option<DateTime<Utc>> {
    history
        .iter()
        .find(|event| predicate(&event.event))
        .map(|event| event.timestamp)
}

fn last_step_sequence(history: &[FlowEventEnvelope], expected_step_id: &str) -> Option<u64> {
    history.iter().rev().find_map(|envelope| {
        let step_id = match &envelope.event {
            FlowEvent::StepCreated { step_id, .. }
            | FlowEvent::StepStarted { step_id, .. }
            | FlowEvent::StepCompleted { step_id, .. }
            | FlowEvent::StepRetrying { step_id, .. }
            | FlowEvent::StepFailed { step_id, .. }
            | FlowEvent::RunRetryExhausted { step_id, .. } => step_id,
            _ => return None,
        };
        (step_id == expected_step_id).then_some(envelope.sequence)
    })
}

#[derive(Clone)]
pub struct WorkflowRunHistoryReader {
    engine: FlowEngine,
}

impl WorkflowRunHistoryReader {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }

    async fn read_page(
        &self,
        flow_run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<WorkflowRunHistoryPage, FlowError> {
        let limit = limit.clamp(1, 100);
        let history = match self.engine.history(flow_run_id).await {
            Ok(history) => history,
            Err(FlowError::RunNotFound(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut selected = history
            .into_iter()
            .filter(|event| event.sequence > after_sequence)
            .take(limit + 1)
            .collect::<Vec<_>>();
        let has_more = selected.len() > limit;
        if has_more {
            selected.pop();
        }
        let events = selected
            .iter()
            .map(summarize_event)
            .collect::<Result<Vec<_>, _>>()
            .map_err(FlowError::Serialization)?;
        let next_sequence = has_more
            .then(|| events.last().map(|event| event.sequence))
            .flatten();
        Ok(WorkflowRunHistoryPage {
            events,
            next_sequence,
        })
    }
}

#[async_trait::async_trait]
impl IWorkflowRunHistoryReader for WorkflowRunHistoryReader {
    async fn read(
        &self,
        flow_run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<WorkflowRunHistoryPage, String> {
        self.read_page(flow_run_id, after_sequence, limit)
            .await
            .map_err(|error| error.to_string())
    }
}

fn summarize_event(
    envelope: &FlowEventEnvelope,
) -> Result<WorkflowRunHistoryEvent, serde_json::Error> {
    let (step_id, attempt, details) = match &envelope.event {
        FlowEvent::RunCreated { spec, input } => {
            let run_input = serde_json::from_value::<WorkflowRunInput>(input.clone()).ok();
            (
                None,
                None,
                json!({
                    "workflowName": spec.name,
                    "workflowVersion": spec.version,
                    "runtimeBuildId": spec.runtime_build_id,
                    "workflowRunId": run_input.as_ref().map(|value| value.workflow_run_id),
                    "planRevisionId": run_input.as_ref().map(|value| value.plan_revision_id),
                    "planDigest": run_input.as_ref().map(|value| &value.plan_digest),
                    "inputDigest": run_input.as_ref().map(|value| &value.plan.input_digest),
                }),
            )
        }
        FlowEvent::RunStarted => (None, None, json!({})),
        FlowEvent::RunCompleted { output } => (None, None, json!({"output": output})),
        FlowEvent::RunFailed { error } => (None, None, json!({"error": error})),
        FlowEvent::RunCancellationRequested { request } => {
            (None, None, json!({"reason": request.reason}))
        }
        FlowEvent::RunCancelled { reason } => (None, None, json!({"reason": reason})),
        FlowEvent::RunTimedOut { deadline, reason } => {
            (None, None, json!({"deadline": deadline, "reason": reason}))
        }
        FlowEvent::RunRetryExhausted {
            step_id,
            attempt,
            error,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error}),
        ),
        FlowEvent::RunHostShutdown { reason } => (None, None, json!({"reason": reason})),
        FlowEvent::RunProgressRecorded { progress } => {
            (None, None, serde_json::to_value(progress)?)
        }
        FlowEvent::ChildOperationLinked { child } => (None, None, serde_json::to_value(child)?),
        FlowEvent::StepCreated {
            step_id,
            step_name,
            input,
            retry,
        } => {
            let canonical = canonical_json_bounded(
                input,
                WORKFLOW_RUN_OUTPUT_MAX_BYTES,
                "Workflow history step input",
            )
            .unwrap_or_default();
            (
                Some(step_id.clone()),
                None,
                json!({
                    "stepName": step_name,
                    "inputDigest": Sha256Digest::parse(sha256_digest(&canonical)).ok(),
                    "retry": retry,
                }),
            )
        }
        FlowEvent::StepStarted { step_id, attempt } => {
            (Some(step_id.clone()), Some(*attempt), json!({}))
        }
        FlowEvent::StepCompleted { step_id, output } => {
            (Some(step_id.clone()), None, json!({"result": output}))
        }
        FlowEvent::StepRetrying {
            step_id,
            attempt,
            error,
            retry_after,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error, "retryAfter": retry_after}),
        ),
        FlowEvent::StepFailed {
            step_id,
            attempt,
            error,
        } => (
            Some(step_id.clone()),
            Some(*attempt),
            json!({"error": error}),
        ),
        FlowEvent::WaitCreated { wait_id, resume_at } => (
            None,
            None,
            json!({"waitId": wait_id, "resumeAt": resume_at}),
        ),
        FlowEvent::WaitCompleted { wait_id } => (None, None, json!({"waitId": wait_id})),
        FlowEvent::HookCreated {
            hook_id, metadata, ..
        } => (None, None, json!({"hookId": hook_id, "metadata": metadata})),
        FlowEvent::HookReceived { hook_id, .. } => (
            None,
            None,
            json!({"hookId": hook_id, "payload": "redacted"}),
        ),
        FlowEvent::HookDisposed { hook_id } => (None, None, json!({"hookId": hook_id})),
    };
    Ok(WorkflowRunHistoryEvent {
        sequence: envelope.sequence,
        event_id: envelope.event_id,
        event_key: envelope.event.event_key().into(),
        occurred_at: envelope.timestamp,
        step_id,
        attempt,
        details,
    })
}
