use super::fixture::{Fixture, EXECUTION_STEP_ID};
use super::{
    crash_at, create_workflow_run, require_child_history_unchanged, require_completed_history,
    require_execution_authority, require_execution_child_reference,
    require_execution_marker_identity, require_execution_output, require_history_unchanged,
    require_run_version, start_idempotency, ProbeMode, RecoveryRuntime, TestResult,
};
use a3s_cloud_control_plane::modules::executions::{
    ExecutionOutcome, ExecutionStatus, EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::operations::OperationStatus;
use a3s_cloud_control_plane::modules::workflow::{
    FlowWorkflowRunCoordinator, IWorkflowRunCoordinator, WorkflowExecutionStepOutput,
    WorkflowRunStatus,
};
use a3s_flow::WorkflowRunStatus as FlowRunStatus;
use chrono::Utc;
use std::sync::Arc;

#[allow(clippy::too_many_lines)]
pub(super) async fn exercise_execution_child_matrix(
    fixture: &Fixture,
    runtime: &RecoveryRuntime,
) -> TestResult<u64> {
    let input = &fixture.document.execution_input;
    let created = create_workflow_run(
        &fixture.executor,
        fixture.document.actor,
        input,
        start_idempotency(input)?,
    )
    .await?;
    if created.replayed
        || created.value.run.status != WorkflowRunStatus::Pending
        || created.value.run.aggregate_version != 1
    {
        return Err("finite Execution WorkflowRun was not created once as Pending".into());
    }
    let start = runtime.operation_reconciler().run_once().await?;
    if start.inspected != 1 || start.projected != 1 || !start.failures.is_empty() {
        return Err(format!(
            "finite Execution parent Operation did not start its existing Flow: {start:#?}"
        )
        .into());
    }
    let initial_snapshot = runtime
        .engine
        .snapshot(&input.workflow_run_id.to_string())
        .await?;
    if initial_snapshot.status != FlowRunStatus::Suspended
        || !initial_snapshot.child_operations.is_empty()
    {
        return Err("finite Execution parent did not suspend at its typed hook".into());
    }

    let committed_marker = crash_at(fixture, 5, ProbeMode::ExecutionChildCommitted).await?;
    let child = runtime
        .executions
        .find_for_workflow(
            input.organization_id,
            input.workflow_run_id,
            EXECUTION_STEP_ID,
            1,
        )
        .await?
        .ok_or("child-committed recovery did not find the finite Execution")?;
    require_execution_marker_identity(
        &committed_marker,
        ProbeMode::ExecutionChildCommitted,
        input,
        &child,
    )?;
    require_execution_authority(input, &child)?;
    if child.status != ExecutionStatus::Queued
        || child.aggregate_version != 1
        || runtime
            .operations
            .find_request(child.operation_id)
            .await?
            .is_some()
    {
        return Err("child-committed boundary crossed into Operation dispatch".into());
    }
    require_run_version(&runtime.runs, input, 1, "finite child commit process death").await?;

    let stored = runtime
        .runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or("finite Execution WorkflowRun disappeared during adoption")?;
    let adopted_projection = FlowWorkflowRunCoordinator::with_executions(
        runtime.engine.clone(),
        Arc::clone(&runtime.execution_port),
    )
    .reconcile(&stored, Utc::now())
    .await?
    .ok_or("finite child adoption did not produce a waiting projection")?;
    let adopted = runtime
        .executions
        .find_for_workflow(
            input.organization_id,
            input.workflow_run_id,
            EXECUTION_STEP_ID,
            1,
        )
        .await?
        .ok_or("finite child disappeared during adoption replay")?;
    if adopted_projection.run.status != WorkflowRunStatus::Waiting || adopted != child {
        return Err("finite child adoption replay changed the exact Execution".into());
    }
    require_run_version(&runtime.runs, input, 1, "finite child adoption replay").await?;

    let enqueue = runtime.execution_reconciler().run_once(100).await?;
    if enqueue.started != 1 || enqueue.replayed != 0 || !enqueue.failures.is_empty() {
        return Err(format!(
            "finite child did not enqueue exactly one existing Operation: {enqueue:#?}"
        )
        .into());
    }
    let stable_enqueue = runtime.execution_reconciler().run_once(100).await?;
    if stable_enqueue.started != 0
        || stable_enqueue.replayed != 0
        || !stable_enqueue.failures.is_empty()
    {
        return Err(format!(
            "finite child remained eligible for duplicate Operation enqueue: {stable_enqueue:#?}"
        )
        .into());
    }
    let child_request = runtime
        .operations
        .find_request(child.operation_id)
        .await?
        .ok_or("finite child Operation request was not persisted")?;
    if child_request.id != child.operation_id
        || child_request.subject.kind() != "execution"
        || child_request.subject.id() != child.id.as_uuid()
        || child_request.workflow.name() != EXECUTION_WORKFLOW_NAME
        || child_request.workflow.version() != EXECUTION_WORKFLOW_VERSION
    {
        return Err("finite child Operation authority drifted during enqueue".into());
    }
    let suspended_parent_projection = runtime
        .operations
        .find_projection(created.value.run.operation_id)
        .await?
        .ok_or("finite parent Operation projection disappeared before starting its child")?;
    let child_start = runtime.operation_reconciler().run_once().await?;
    if child_start.inspected != 2 || child_start.projected != 1 || !child_start.failures.is_empty()
    {
        return Err(format!(
            "finite child Operation was not projected beside one stable parent replay: {child_start:#?}"
        )
        .into());
    }
    let parent_projection = runtime
        .operations
        .find_projection(created.value.run.operation_id)
        .await?
        .ok_or("finite parent Operation projection disappeared while starting its child")?;
    if parent_projection != suspended_parent_projection {
        return Err(format!(
            "finite parent Operation projection changed during a stable replay: before={suspended_parent_projection:#?} after={parent_projection:#?}",
        )
        .into());
    }
    let child_projection = runtime
        .operations
        .find_projection(child.operation_id)
        .await?
        .ok_or("finite child Operation projection was not persisted")?;
    if child_projection.status != OperationStatus::Succeeded {
        return Err(format!(
            "finite child Operation projected {:?} instead of succeeded",
            child_projection.status
        )
        .into());
    }
    let child_history = runtime
        .engine
        .history(&child.operation_id.to_string())
        .await?;
    require_completed_history(&child_history)?;

    let linked_marker = crash_at(fixture, 6, ProbeMode::ExecutionChildLinked).await?;
    require_execution_marker_identity(
        &linked_marker,
        ProbeMode::ExecutionChildLinked,
        input,
        &child,
    )?;
    let linked_snapshot = runtime
        .engine
        .snapshot(&input.workflow_run_id.to_string())
        .await?;
    require_execution_child_reference(input, &child, &linked_snapshot)?;
    require_run_version(&runtime.runs, input, 1, "finite child link process death").await?;
    let linked_history = runtime
        .engine
        .history(&input.workflow_run_id.to_string())
        .await?;
    let linked_replay = FlowWorkflowRunCoordinator::with_executions(
        runtime.engine.clone(),
        Arc::clone(&runtime.execution_port),
    )
    .reconcile(&stored, Utc::now())
    .await?
    .ok_or("finite child link replay did not produce a waiting projection")?;
    if linked_replay.run.status != WorkflowRunStatus::Waiting {
        return Err("finite child link replay changed the suspended parent state".into());
    }
    require_history_unchanged(
        &runtime.engine,
        input,
        &linked_history,
        "finite child link replay",
    )
    .await?;

    let mut cleaning = runtime
        .executions
        .find(input.organization_id, child.id)
        .await?
        .ok_or("finite child disappeared before cleanup")?;
    let expected_version = cleaning.aggregate_version;
    cleaning.begin_cleanup(
        ExecutionOutcome::Succeeded { exit_code: 0 },
        cleaning.updated_at + chrono::Duration::milliseconds(1),
    )?;
    cleaning = runtime.executions.save(cleaning, expected_version).await?;
    if cleaning.status != ExecutionStatus::CleanupPending {
        return Err("finite child skipped cleanup-pending authority".into());
    }
    let expected_version = cleaning.aggregate_version;
    cleaning.complete_cleanup(cleaning.updated_at + chrono::Duration::milliseconds(1))?;
    let succeeded = runtime.executions.save(cleaning, expected_version).await?;
    if succeeded.status != ExecutionStatus::Succeeded
        || succeeded.outcome != Some(ExecutionOutcome::Succeeded { exit_code: 0 })
        || succeeded.finished_at.is_none()
    {
        return Err("finite child did not complete cleanup-first success".into());
    }

    let resumed_marker = crash_at(fixture, 7, ProbeMode::ExecutionTerminalResumed).await?;
    require_execution_marker_identity(
        &resumed_marker,
        ProbeMode::ExecutionTerminalResumed,
        input,
        &succeeded,
    )?;
    require_run_version(
        &runtime.runs,
        input,
        1,
        "finite terminal resume process death",
    )
    .await?;
    let terminal_snapshot = runtime
        .engine
        .snapshot(&input.workflow_run_id.to_string())
        .await?;
    if terminal_snapshot.status != FlowRunStatus::Completed {
        return Err("finite terminal child did not complete the parent Flow".into());
    }
    require_execution_child_reference(input, &succeeded, &terminal_snapshot)?;
    let terminal_history = runtime
        .engine
        .history(&input.workflow_run_id.to_string())
        .await?;
    require_completed_history(&terminal_history)?;

    let recovery = runtime.workflow_reconciler()?.run_once(100).await?;
    if recovery.inspected != 1
        || recovery.projected != 1
        || recovery.deferred != 0
        || !recovery.failures.is_empty()
    {
        return Err(format!(
            "finite terminal child projection was not recovered exactly once: {recovery:#?}"
        )
        .into());
    }
    let completed = runtime
        .runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or("finite terminal WorkflowRun projection disappeared")?;
    if completed.run.status != WorkflowRunStatus::Completed
        || completed.run.aggregate_version != 2
        || completed.run.last_flow_sequence != terminal_snapshot.last_sequence
    {
        return Err(format!(
            "finite terminal WorkflowRun projection drifted: {:?}",
            completed.run
        )
        .into());
    }
    let output: WorkflowExecutionStepOutput = serde_json::from_value(
        completed
            .run
            .output
            .clone()
            .ok_or("finite terminal WorkflowRun lost its child output")?,
    )?;
    require_execution_output(input, &succeeded, &output)?;

    let parent_operation = runtime.operation_reconciler().run_once().await?;
    if parent_operation.inspected != 1
        || parent_operation.projected != 1
        || !parent_operation.failures.is_empty()
    {
        return Err(format!(
            "finite parent Operation did not project terminal recovery: {parent_operation:#?}"
        )
        .into());
    }
    let projection = runtime
        .operations
        .find_projection(completed.run.operation_id)
        .await?
        .ok_or("finite parent Operation projection disappeared")?;
    if projection.status != OperationStatus::Succeeded {
        return Err("finite parent Operation did not finish succeeded".into());
    }
    let stable_version = completed.run.aggregate_version;
    let stable_workflow = runtime.workflow_reconciler()?.run_once(100).await?;
    let stable_operation = runtime.operation_reconciler().run_once().await?;
    if stable_workflow.inspected != 0 || stable_operation.inspected != 0 {
        return Err(format!(
            "finite terminal recovery remained eligible: workflow={stable_workflow:#?}, operation={stable_operation:#?}"
        )
        .into());
    }
    require_run_version(
        &runtime.runs,
        input,
        stable_version,
        "finite terminal projection replay",
    )
    .await?;
    require_history_unchanged(
        &runtime.engine,
        input,
        &terminal_history,
        "finite terminal projection replay",
    )
    .await?;
    require_child_history_unchanged(
        &runtime.engine,
        &succeeded,
        &child_history,
        "finite child terminal replay",
    )
    .await?;
    Ok(stable_version)
}
