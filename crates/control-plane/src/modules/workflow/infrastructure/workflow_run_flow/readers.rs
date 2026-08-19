use super::projection::{completed_workflow_steps, verify_flow_authority};
use crate::modules::shared_kernel::domain::{canonical_json_bounded, sha256_digest, Sha256Digest};
use crate::modules::workflow::domain::{
    inspect_workflow_run_variables, inspect_workflow_run_variables_with_composites,
    IWorkflowRunHistoryReader, IWorkflowRunVariableReader, WorkflowRunHistoryEvent,
    WorkflowRunHistoryPage, WorkflowRunInput, WorkflowRunRecord, WorkflowRunVariableInspection,
    WORKFLOW_RUN_OUTPUT_MAX_BYTES,
};
use a3s_flow::{FlowEngine, FlowError, FlowEvent, FlowEventEnvelope};
#[cfg(test)]
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct WorkflowRunVariableReader {
    engine: FlowEngine,
}

impl WorkflowRunVariableReader {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }

    async fn inspect_record(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunVariableInspection, FlowError> {
        for attempt in 0..3 {
            let snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
                Ok(snapshot) => snapshot,
                Err(FlowError::RunNotFound(_)) if record.run.last_flow_sequence == 0 => {
                    return inspect_workflow_run_variables(
                        record,
                        0,
                        record.run.requested_at,
                        &BTreeMap::new(),
                    )
                    .map_err(FlowError::Runtime)
                }
                Err(error) => return Err(error),
            };
            let history = self.engine.history(&record.run.flow_run_id).await?;
            if history.last().map(|event| event.sequence) != Some(snapshot.last_sequence) {
                if attempt < 2 {
                    tokio::task::yield_now().await;
                    continue;
                }
                return Err(FlowError::Runtime(
                    "Workflow variable inspection observed concurrent Flow transitions".into(),
                ));
            }
            verify_flow_authority(record, &snapshot, &history).map_err(FlowError::Runtime)?;
            if snapshot.last_sequence < record.run.last_flow_sequence {
                return Err(FlowError::Runtime(
                    "Workflow variable inspection precedes the persisted Flow projection".into(),
                ));
            }
            let resolved_steps = record
                .run
                .execution_input
                .resolved_steps()
                .map_err(FlowError::Runtime)?;
            let completed =
                completed_workflow_steps(&record.run.execution_input, &resolved_steps, &snapshot)
                    .map_err(FlowError::Runtime)?
                    .completed;
            let outputs = completed
                .iter()
                .map(|(step_id, result)| (step_id.clone(), result.output.clone()))
                .collect::<BTreeMap<_, _>>();
            let composites = completed
                .into_iter()
                .filter_map(|(step_id, result)| {
                    result
                        .composite_region_result
                        .map(|region| (step_id, region))
                })
                .collect::<BTreeMap<_, _>>();
            let observed_at = history
                .last()
                .map(|event| event.timestamp)
                .ok_or_else(|| FlowError::Runtime("WorkflowRun Flow history is empty".into()))?;
            return inspect_workflow_run_variables_with_composites(
                record,
                snapshot.last_sequence,
                observed_at,
                &outputs,
                &composites,
            )
            .map_err(FlowError::Runtime);
        }
        Err(FlowError::Runtime(
            "Workflow variable inspection exhausted its observation attempts".into(),
        ))
    }
}

#[async_trait::async_trait]
impl IWorkflowRunVariableReader for WorkflowRunVariableReader {
    async fn inspect(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunVariableInspection, String> {
        self.inspect_record(record)
            .await
            .map_err(|error| error.to_string())
    }
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
        FlowEvent::RunContinuedAsNew {
            successor_run_id, ..
        } => (
            None,
            None,
            json!({
                "successorRunId": successor_run_id,
                "input": "redacted",
            }),
        ),
        FlowEvent::RunProgressRecorded { progress } => {
            (None, None, serde_json::to_value(progress)?)
        }
        FlowEvent::ChildOperationLinked { child } => (None, None, serde_json::to_value(child)?),
        FlowEvent::ChildWorkflowRequested {
            child_id,
            child_run_id,
            spec,
            cancellation_policy,
            ..
        } => (
            None,
            None,
            json!({
                "childId": child_id,
                "childRunId": child_run_id,
                "workflowName": spec.name,
                "workflowVersion": spec.version,
                "runtimeBuildId": spec.runtime_build_id,
                "cancellationPolicy": cancellation_policy,
                "input": "redacted",
            }),
        ),
        FlowEvent::ChildWorkflowResolved { child_id, outcome } => {
            (None, None, json!({"childId": child_id, "outcome": outcome}))
        }
        FlowEvent::SignalReceived { signal } => (
            None,
            None,
            json!({
                "signalId": signal.signal_id,
                "name": signal.name,
                "payload": "redacted",
            }),
        ),
        FlowEvent::SignalWaitCreated {
            wait_id,
            signal_name,
        } => (
            None,
            None,
            json!({"waitId": wait_id, "signalName": signal_name}),
        ),
        FlowEvent::SignalWaitCompleted { wait_id, signal_id } => (
            None,
            None,
            json!({"waitId": wait_id, "signalId": signal_id}),
        ),
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
        event => (
            None,
            None,
            json!({
                "eventKey": event.event_key(),
                "projection": "unsupported"
            }),
        ),
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

#[cfg(test)]
mod history_summary_tests {
    use super::*;
    use a3s_flow::WorkflowSignal;
    use uuid::Uuid;

    fn envelope(event: FlowEvent) -> FlowEventEnvelope {
        FlowEventEnvelope::new("run-1", 1, Uuid::now_v7(), Utc::now(), event)
    }

    #[test]
    fn signal_history_preserves_identity_without_exposing_payload() {
        let summary = summarize_event(&envelope(FlowEvent::SignalReceived {
            signal: WorkflowSignal::new(
                "signal-1",
                "approval.received",
                json!({"secret": "must-not-leak"}),
            ),
        }))
        .expect("signal history summary");

        assert_eq!(summary.event_key, "flow.signal.received");
        assert_eq!(summary.details["signalId"], "signal-1");
        assert_eq!(summary.details["name"], "approval.received");
        assert_eq!(summary.details["payload"], "redacted");
        assert!(!summary.details.to_string().contains("must-not-leak"));
    }

    #[test]
    fn continuation_history_preserves_successor_without_exposing_input() {
        let summary = summarize_event(&envelope(FlowEvent::RunContinuedAsNew {
            successor_run_id: "run-2".into(),
            input: json!({"secret": "must-not-leak"}),
        }))
        .expect("continuation history summary");

        assert_eq!(summary.event_key, "flow.run.continued_as_new");
        assert_eq!(summary.details["successorRunId"], "run-2");
        assert_eq!(summary.details["input"], "redacted");
        assert!(!summary.details.to_string().contains("must-not-leak"));
    }
}
