use super::projection::verify_flow_authority;
use crate::modules::workflow::domain::{
    IWorkflowRunDiagnosticsReader, WorkflowRunDiagnostic, WorkflowRunDiagnosticCode,
    WorkflowRunDiagnosticSeverity, WorkflowRunDiagnosticStatus, WorkflowRunDiagnostics,
    WorkflowRunEvidenceCorrelation, WorkflowRunFlowStatistics, WorkflowRunObservedFlowStatus,
    WorkflowRunRecord, WorkflowRunStatus, WorkflowRunStepStatistics, WorkflowStepProjectionStatus,
    WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES, WORKFLOW_RUN_DIAGNOSTICS_SCHEMA,
};
use a3s_flow::{
    FlowEngine, FlowError, FlowEvent, FlowEventEnvelope, HookStatus, WaitStatus,
    WorkflowRunSnapshot, WorkflowRunStatus as FlowRunStatus, WorkflowTerminalOutcome,
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct WorkflowRunDiagnosticsReader {
    engine: FlowEngine,
}

impl WorkflowRunDiagnosticsReader {
    pub const fn new(engine: FlowEngine) -> Self {
        Self { engine }
    }

    async fn inspect_record(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowRunDiagnostics, FlowError> {
        record.validate().map_err(FlowError::Runtime)?;
        for attempt in 0..3 {
            let snapshot = match self.engine.snapshot(&record.run.flow_run_id).await {
                Ok(snapshot) => snapshot,
                Err(FlowError::RunNotFound(_)) => {
                    return build_diagnostics(record, None, &[]).map_err(FlowError::Runtime);
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
                    "Workflow diagnostics observed concurrent Flow transitions".into(),
                ));
            }
            verify_flow_authority(record, &snapshot, &history).map_err(FlowError::Runtime)?;
            return build_diagnostics(record, Some(&snapshot), &history)
                .map_err(FlowError::Runtime);
        }
        Err(FlowError::Runtime(
            "Workflow diagnostics exhausted its observation attempts".into(),
        ))
    }
}

#[async_trait::async_trait]
impl IWorkflowRunDiagnosticsReader for WorkflowRunDiagnosticsReader {
    async fn inspect(&self, record: &WorkflowRunRecord) -> Result<WorkflowRunDiagnostics, String> {
        self.inspect_record(record)
            .await
            .map_err(|error| error.to_string())
    }
}

fn build_diagnostics(
    record: &WorkflowRunRecord,
    snapshot: Option<&WorkflowRunSnapshot>,
    history: &[FlowEventEnvelope],
) -> Result<WorkflowRunDiagnostics, String> {
    let step_statistics = step_statistics(record);
    let (evidence_correlations, evidence_correlations_truncated) = evidence_correlations(record);
    let flow_statistics = flow_statistics(snapshot, history);
    let observed_flow_sequence = snapshot.map(|value| value.last_sequence);
    let unprojected_event_count = observed_flow_sequence
        .unwrap_or_default()
        .saturating_sub(record.run.last_flow_sequence);
    let observed_flow_status = snapshot
        .map(|value| observed_flow_status(value.status))
        .transpose()?
        .unwrap_or(WorkflowRunObservedFlowStatus::Missing);
    let flow_runtime_build_id = snapshot
        .and_then(|value| value.spec.runtime_build_id.as_ref())
        .map(|value| value.as_str().to_owned())
        .or_else(|| record.run.flow_runtime_build_id.clone());
    let observed_at = history
        .last()
        .map(|event| event.timestamp)
        .unwrap_or(record.run.updated_at);
    let diagnostics =
        diagnostic_entries(record, snapshot, &flow_statistics, observed_flow_sequence);
    let diagnostic_status = if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == WorkflowRunDiagnosticSeverity::Error)
    {
        WorkflowRunDiagnosticStatus::Error
    } else if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == WorkflowRunDiagnosticSeverity::Warning)
    {
        WorkflowRunDiagnosticStatus::Attention
    } else {
        WorkflowRunDiagnosticStatus::Ok
    };
    Ok(WorkflowRunDiagnostics {
        schema: WORKFLOW_RUN_DIAGNOSTICS_SCHEMA.into(),
        workflow_run_id: record.run.id,
        operation_id: record.run.operation_id,
        flow_run_id: record.run.flow_run_id.clone(),
        run_status: record.run.status,
        observed_flow_status,
        flow_runtime_build_id,
        projected_flow_sequence: record.run.last_flow_sequence,
        observed_flow_sequence,
        unprojected_event_count,
        observed_at,
        step_statistics,
        flow_statistics,
        evidence_correlations,
        evidence_correlations_truncated,
        diagnostic_status,
        diagnostics,
    })
}

fn observed_flow_status(status: FlowRunStatus) -> Result<WorkflowRunObservedFlowStatus, String> {
    match status {
        FlowRunStatus::Pending => Ok(WorkflowRunObservedFlowStatus::Pending),
        FlowRunStatus::Running => Ok(WorkflowRunObservedFlowStatus::Running),
        FlowRunStatus::Suspended => Ok(WorkflowRunObservedFlowStatus::Suspended),
        FlowRunStatus::Cancelling => Ok(WorkflowRunObservedFlowStatus::Cancelling),
        FlowRunStatus::Completed => Ok(WorkflowRunObservedFlowStatus::Completed),
        FlowRunStatus::Failed => Ok(WorkflowRunObservedFlowStatus::Failed),
        FlowRunStatus::Cancelled => Ok(WorkflowRunObservedFlowStatus::Cancelled),
        FlowRunStatus::ContinuedAsNew => Ok(WorkflowRunObservedFlowStatus::ContinuedAsNew),
        status => Err(format!(
            "Workflow diagnostics do not support Flow status {status:?}"
        )),
    }
}

fn step_statistics(record: &WorkflowRunRecord) -> WorkflowRunStepStatistics {
    let mut statistics = WorkflowRunStepStatistics {
        total: count(record.steps.len()),
        pending: 0,
        running: 0,
        completed: 0,
        failed: 0,
        cancelled: 0,
        skipped: 0,
        total_attempt_generations: 0,
        evidence_reference_count: 0,
    };
    for step in &record.steps {
        match step.status {
            WorkflowStepProjectionStatus::Pending => statistics.pending += 1,
            WorkflowStepProjectionStatus::Running => statistics.running += 1,
            WorkflowStepProjectionStatus::Completed => statistics.completed += 1,
            WorkflowStepProjectionStatus::Failed => statistics.failed += 1,
            WorkflowStepProjectionStatus::Cancelled => statistics.cancelled += 1,
            WorkflowStepProjectionStatus::Skipped => statistics.skipped += 1,
        }
        statistics.total_attempt_generations += u64::from(step.attempt_generation);
        statistics.evidence_reference_count += count(step.evidence_references.len());
    }
    statistics
}

fn flow_statistics(
    snapshot: Option<&WorkflowRunSnapshot>,
    history: &[FlowEventEnvelope],
) -> WorkflowRunFlowStatistics {
    let mut event_counts = BTreeMap::new();
    let mut retry_event_count = 0;
    let mut host_shutdown_count = 0;
    for envelope in history {
        *event_counts
            .entry(envelope.event.event_key().to_owned())
            .or_insert(0) += 1;
        if matches!(envelope.event, FlowEvent::StepRetrying { .. }) {
            retry_event_count += 1;
        }
        if matches!(envelope.event, FlowEvent::RunHostShutdown { .. }) {
            host_shutdown_count += 1;
        }
    }
    WorkflowRunFlowStatistics {
        event_count: count(history.len()),
        event_counts,
        durable_step_count: snapshot.map_or(0, |value| count(value.steps.len())),
        active_hook_count: snapshot.map_or(0, |value| {
            count(
                value
                    .hooks
                    .values()
                    .filter(|hook| hook.status == HookStatus::Active)
                    .count(),
            )
        }),
        pending_timer_count: snapshot.map_or(0, |value| {
            count(
                value
                    .waits
                    .values()
                    .filter(|wait| wait.status == WaitStatus::Waiting)
                    .count(),
            )
        }),
        linked_child_operation_count: snapshot
            .map_or(0, |value| count(value.child_operations.len())),
        child_workflow_count: snapshot.map_or(0, |value| count(value.child_workflows.len())),
        retry_event_count,
        host_shutdown_count,
    }
}

fn evidence_correlations(
    record: &WorkflowRunRecord,
) -> (Vec<WorkflowRunEvidenceCorrelation>, bool) {
    let total = record
        .steps
        .iter()
        .map(|step| step.evidence_references.len())
        .sum::<usize>();
    let mut remaining = WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES;
    let mut steps = record.steps.iter().collect::<Vec<_>>();
    steps.sort_by(|left, right| left.step_id.cmp(&right.step_id));
    let mut correlations = Vec::new();
    for step in steps {
        if remaining == 0 {
            break;
        }
        let references = step
            .evidence_references
            .iter()
            .take(remaining)
            .cloned()
            .collect::<Vec<_>>();
        if references.is_empty() {
            continue;
        }
        remaining -= references.len();
        correlations.push(WorkflowRunEvidenceCorrelation {
            step_id: step.step_id.clone(),
            references,
        });
    }
    (
        correlations,
        total > WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES,
    )
}

fn diagnostic_entries(
    record: &WorkflowRunRecord,
    snapshot: Option<&WorkflowRunSnapshot>,
    flow: &WorkflowRunFlowStatistics,
    observed_sequence: Option<u64>,
) -> Vec<WorkflowRunDiagnostic> {
    let mut diagnostics = Vec::new();
    if snapshot.is_none() {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::FlowHistoryMissing,
            if record.run.last_flow_sequence == 0 {
                WorkflowRunDiagnosticSeverity::Warning
            } else {
                WorkflowRunDiagnosticSeverity::Error
            },
            "The correlated A3S Flow history is not available.",
        ));
    }
    if observed_sequence.is_some_and(|sequence| sequence > record.run.last_flow_sequence) {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::ProjectionLag,
            WorkflowRunDiagnosticSeverity::Warning,
            "The persisted Workflow projection is behind the observed A3S Flow history.",
        ));
    }
    if observed_sequence.is_some_and(|sequence| sequence < record.run.last_flow_sequence) {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::ProjectionAhead,
            WorkflowRunDiagnosticSeverity::Error,
            "The persisted Workflow projection is ahead of the observed A3S Flow history.",
        ));
    }
    if flow.active_hook_count > 0 || flow.pending_timer_count > 0 {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::ActiveExternalWait,
            WorkflowRunDiagnosticSeverity::Info,
            "The run has an active durable external hook or timer.",
        ));
    }
    if record.run.status == WorkflowRunStatus::Cancelling
        || snapshot.is_some_and(|value| value.status == FlowRunStatus::Cancelling)
    {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::CancellationPending,
            WorkflowRunDiagnosticSeverity::Info,
            "Cancellation is waiting for the durable cleanup path to settle.",
        ));
    }
    if flow.retry_event_count > 0 {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::RetryObserved,
            WorkflowRunDiagnosticSeverity::Info,
            "One or more durable step retries were observed.",
        ));
    }
    if flow.host_shutdown_count > 0 {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::RuntimeRecoveryObserved,
            WorkflowRunDiagnosticSeverity::Info,
            "The Flow history contains a runtime host-shutdown recovery boundary.",
        ));
    }
    if record
        .steps
        .iter()
        .any(|step| step.status == WorkflowStepProjectionStatus::Failed)
    {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::StepFailureObserved,
            WorkflowRunDiagnosticSeverity::Warning,
            "One or more Workflow step projections are failed.",
        ));
    }
    let timed_out = record.run.status == WorkflowRunStatus::TimedOut
        || snapshot.is_some_and(|value| {
            matches!(
                value.terminal_outcome,
                Some(WorkflowTerminalOutcome::TimedOut { .. })
            )
        });
    let failed = !timed_out
        && (record.run.status == WorkflowRunStatus::Failed
            || snapshot.is_some_and(|value| value.status == FlowRunStatus::Failed));
    let cancelled = record.run.status == WorkflowRunStatus::Cancelled
        || snapshot.is_some_and(|value| value.status == FlowRunStatus::Cancelled);
    if failed {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::RunFailed,
            WorkflowRunDiagnosticSeverity::Error,
            "The Workflow run terminated with a failure.",
        ));
    }
    if timed_out {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::RunTimedOut,
            WorkflowRunDiagnosticSeverity::Error,
            "The Workflow run exceeded its immutable deadline.",
        ));
    }
    if cancelled {
        diagnostics.push(diagnostic(
            WorkflowRunDiagnosticCode::RunCancelled,
            WorkflowRunDiagnosticSeverity::Info,
            "The Workflow run completed cancellation.",
        ));
    }
    diagnostics
}

fn diagnostic(
    code: WorkflowRunDiagnosticCode,
    severity: WorkflowRunDiagnosticSeverity,
    message: &str,
) -> WorkflowRunDiagnostic {
    WorkflowRunDiagnostic {
        code,
        severity,
        message: message.into(),
    }
}

fn count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::shared_kernel::domain::PrincipalId;
    use crate::modules::workflow::test_support::workflow_run_input;
    use crate::modules::workflow::{WorkflowRun, WorkflowRunFlowRuntime};
    use a3s_flow::{RuntimeBuildCompatibility, RuntimeBuildId, WorkflowSpec};

    #[tokio::test]
    async fn reports_missing_flow_history_without_losing_persisted_authority() {
        let input = workflow_run_input().expect("Workflow input");
        let (run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
        let record = WorkflowRunRecord { run, steps };
        let reader = WorkflowRunDiagnosticsReader::new(FlowEngine::in_memory(std::sync::Arc::new(
            WorkflowRunFlowRuntime::default(),
        )));

        let diagnostics = reader.inspect(&record).await.expect("diagnostics");

        assert_eq!(diagnostics.schema, WORKFLOW_RUN_DIAGNOSTICS_SCHEMA);
        assert_eq!(
            diagnostics.observed_flow_status,
            WorkflowRunObservedFlowStatus::Missing
        );
        assert_eq!(diagnostics.observed_flow_sequence, None);
        assert_eq!(diagnostics.step_statistics.total, 6);
        assert_eq!(
            diagnostics.diagnostic_status,
            WorkflowRunDiagnosticStatus::Attention
        );
        assert_eq!(
            diagnostics.diagnostics[0].code,
            WorkflowRunDiagnosticCode::FlowHistoryMissing
        );
    }

    #[tokio::test]
    async fn reports_consistent_flow_statistics_and_projection_lag() -> Result<(), FlowError> {
        let mut input = workflow_run_input().map_err(FlowError::Runtime)?;
        input.requested_at = chrono::Utc::now();
        input.deadline_at = input.requested_at + chrono::Duration::hours(1);
        input.validate().map_err(FlowError::Runtime)?;
        let run_id = input.workflow_run_id.to_string();
        let (run, steps) =
            WorkflowRun::create(input.clone(), PrincipalId::new()).map_err(FlowError::Runtime)?;
        let record = WorkflowRunRecord { run, steps };
        let runtime_build_id = RuntimeBuildId::new("a3s-cloud-workflow-diagnostics-test@1")?;
        let engine = FlowEngine::builder(std::sync::Arc::new(WorkflowRunFlowRuntime::default()))
            .with_runtime_build_compatibility(RuntimeBuildCompatibility::new(
                runtime_build_id.clone(),
            ))
            .build();
        engine
            .start_with_id(
                &run_id,
                WorkflowSpec::rust_embedded(
                    &input.flow_workflow_name,
                    &input.flow_workflow_version,
                    "a3s-cloud",
                    "main",
                )
                .with_runtime_build(runtime_build_id),
                serde_json::to_value(&input)?,
            )
            .await?;

        let diagnostics = WorkflowRunDiagnosticsReader::new(engine)
            .inspect(&record)
            .await
            .map_err(FlowError::Runtime)?;

        assert_eq!(
            diagnostics.observed_flow_status,
            WorkflowRunObservedFlowStatus::Completed
        );
        assert!(diagnostics
            .observed_flow_sequence
            .is_some_and(|value| value > 0));
        assert!(diagnostics.unprojected_event_count > 0);
        assert!(diagnostics.flow_statistics.event_count > 0);
        assert_eq!(
            diagnostics.flow_statistics.event_count,
            diagnostics
                .flow_statistics
                .event_counts
                .values()
                .sum::<u64>()
        );
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|entry| entry.code == WorkflowRunDiagnosticCode::ProjectionLag));
        Ok(())
    }

    #[test]
    fn caps_evidence_correlations_and_reports_truncation() {
        let input = workflow_run_input().expect("Workflow input");
        let (run, steps) = WorkflowRun::create(input, PrincipalId::new()).expect("WorkflowRun");
        let template = steps.first().expect("step projection");
        let mut expanded = Vec::new();
        for step_index in 0_u128..9 {
            let mut step = template.clone();
            step.step_id = format!("step-{step_index:02}");
            step.evidence_references = (0_u128..32)
                .map(|reference_index| {
                    format!(
                        "urn:a3s:cloud:operations:operation:{}",
                        uuid::Uuid::from_u128(step_index * 32 + reference_index + 1)
                    )
                })
                .collect();
            expanded.push(step);
        }
        let record = WorkflowRunRecord {
            run,
            steps: expanded,
        };

        let (correlations, truncated) = evidence_correlations(&record);

        assert!(truncated);
        assert_eq!(correlations.len(), 8);
        assert_eq!(
            correlations
                .iter()
                .map(|correlation| correlation.references.len())
                .sum::<usize>(),
            WORKFLOW_RUN_DIAGNOSTICS_MAX_EVIDENCE_REFERENCES
        );
    }
}
