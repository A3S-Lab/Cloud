use super::fixture::Fixture;
use super::process::ProbeMode;
use super::TestResult;
use a3s_cloud_control_plane::modules::workflow::{
    IWorkflowRunRepository, WorkflowRunInput, WorkflowRunRecord,
};
use a3s_flow::{FlowEngine, FlowEvent, FlowEventEnvelope};
use std::sync::Arc;

pub(super) fn require_marker_identity(
    marker: &serde_json::Value,
    mode: ProbeMode,
    input: &WorkflowRunInput,
) -> TestResult {
    let expected_run = input.workflow_run_id.to_string();
    if marker["mode"].as_str() != Some(mode.as_str())
        || marker["workflowRunId"].as_str() != Some(expected_run.as_str())
        || marker["operationId"].as_str() != Some(expected_run.as_str())
        || marker["flowRunId"].as_str() != Some(expected_run.as_str())
        || marker["aggregateVersion"].as_u64().is_none()
    {
        return Err(format!(
            "WorkflowRun crash marker did not bind {} to its durable identities: {marker}",
            mode.as_str()
        )
        .into());
    }
    Ok(())
}

pub(super) fn require_run_identity(
    record: &WorkflowRunRecord,
    input: &WorkflowRunInput,
) -> TestResult {
    if record.run.id != input.workflow_run_id
        || record.run.operation_id.as_uuid() != input.workflow_run_id.as_uuid()
        || record.run.flow_run_id != input.workflow_run_id.to_string()
        || record.run.execution_input != *input
    {
        return Err("WorkflowRun, Operation, Flow, or immutable input identity drifted".into());
    }
    Ok(())
}

pub(super) fn require_completed_history(history: &[FlowEventEnvelope]) -> TestResult {
    let created = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCreated { .. }))
        .count();
    let started = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunStarted))
        .count();
    let completed = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCompleted { .. }))
        .count();
    if (created, started, completed) != (1, 1, 1) {
        return Err(format!(
            "completed Flow history was duplicated: created={created}, started={started}, completed={completed}"
        )
        .into());
    }
    Ok(())
}

pub(super) fn require_cancellation_history(history: &[FlowEventEnvelope]) -> TestResult {
    let requested = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCancellationRequested { .. }))
        .count();
    let cancelled = history
        .iter()
        .filter(|event| matches!(event.event, FlowEvent::RunCancelled { .. }))
        .count();
    if (requested, cancelled) != (1, 1) {
        return Err(format!(
            "Flow cancellation history was not exact once: requested={requested}, cancelled={cancelled}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn require_history_unchanged(
    engine: &FlowEngine,
    input: &WorkflowRunInput,
    before: &[FlowEventEnvelope],
    label: &str,
) -> TestResult {
    let after = engine.history(&input.workflow_run_id.to_string()).await?;
    if after != before {
        return Err(format!("{label} appended duplicate Flow history").into());
    }
    Ok(())
}

pub(super) async fn require_run_version(
    repository: &Arc<dyn IWorkflowRunRepository>,
    input: &WorkflowRunInput,
    expected: u64,
    label: &str,
) -> TestResult {
    let actual = repository
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or_else(|| format!("WorkflowRun disappeared during {label}"))?
        .run
        .aggregate_version;
    if actual != expected {
        return Err(format!(
            "WorkflowRun aggregate version changed during {label}: expected {expected}, got {actual}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn verify_database_evidence(fixture: &Fixture) -> TestResult {
    let connection = fixture.executor.pool().get().await?;
    let terminal_id = fixture.document.terminal_input.workflow_run_id.as_uuid();
    let cancellation_id = fixture
        .document
        .cancellation_input
        .workflow_run_id
        .as_uuid();
    let row = connection
        .query_one(
            "select \
                (select count(*) from workflow_runs where id in ($1, $2)), \
                (select count(*) from operation_requests where operation_id in ($1, $2)), \
                (select count(*) from operation_projections where operation_id in ($1, $2)), \
                (select count(*) from outbox_events where aggregate_id in ($1, $2) and event_key = 'workflow.run.requested'), \
                (select count(*) from outbox_events where aggregate_id = $2 and event_key = 'workflow.run.cancellation.requested')",
            &[&terminal_id, &cancellation_id],
        )
        .await?;
    let evidence = (
        row.get::<_, i64>(0),
        row.get::<_, i64>(1),
        row.get::<_, i64>(2),
        row.get::<_, i64>(3),
        row.get::<_, i64>(4),
    );
    if evidence != (2, 2, 2, 2, 1) {
        return Err(format!(
            "WorkflowRun process-death relational evidence was not exact once: {evidence:?}"
        )
        .into());
    }
    Ok(())
}
