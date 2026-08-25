use super::fixture::{Fixture, ProbeDocument};
use super::process::{CompositeChildMarker, CrashMarker, ProbeMode};
use super::{ProcessDeathFlowRuntime, RecoveryRuntime, TestResult};
use a3s_cloud_control_plane::infrastructure::FlowInfrastructure;
use a3s_cloud_control_plane::modules::executions::{
    IExecutionRepository, IExecutionTemplateRepository, IWorkflowExecutionPort,
    PostgresExecutionRepository, PostgresExecutionTemplateRepository,
    WorkflowExecutionApplicationService,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::workflow::domain::{
    WorkflowCompositeFrame, WorkflowCompositeHookMetadata, WorkflowCompositeRegions,
    WorkflowCompositeWaveHookMetadata, WorkflowVariableContract, WorkflowVariableDefaults,
};
use a3s_cloud_control_plane::modules::workflow::{
    FlowWorkflowRunCoordinator, IOntologyRepository, IWorkflowCompositeExecutionPort,
    IWorkflowDefinitionRepository, IWorkflowGoalRepository, IWorkflowRunCoordinator,
    IWorkflowRunRepository, PostgresOntologyRepository, PostgresWorkflowDefinitionRepository,
    PostgresWorkflowGoalRepository, PostgresWorkflowRunRepository,
    WorkflowCompositeChildReferenceMetadata, WorkflowCompositeExecutionApplicationService,
    WorkflowRunInput, WorkflowRunRecord, WorkflowRunStatus,
    WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA,
};
use a3s_flow::{FlowEngine, WorkflowRunSnapshot, WorkflowRunStatus as FlowRunStatus};
use a3s_orm::PostgresExecutor;
use chrono::Utc;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CompositeScenario {
    Loop,
    Iteration,
}

impl CompositeScenario {
    const fn label(self) -> &'static str {
        match self {
            Self::Loop => "Loop",
            Self::Iteration => "Iteration",
        }
    }

    const fn child_count(self) -> usize {
        match self {
            Self::Loop => 1,
            Self::Iteration => 2,
        }
    }

    const fn committed_mode(self) -> ProbeMode {
        match self {
            Self::Loop => ProbeMode::LoopChildCommitted,
            Self::Iteration => ProbeMode::IterationChildrenCommitted,
        }
    }

    const fn terminal_mode(self) -> ProbeMode {
        match self {
            Self::Loop => ProbeMode::LoopTerminalResumed,
            Self::Iteration => ProbeMode::IterationTerminalResumed,
        }
    }

    fn input(self, document: &ProbeDocument) -> &WorkflowRunInput {
        match self {
            Self::Loop => &document.loop_input,
            Self::Iteration => &document.iteration_input,
        }
    }
}

pub(super) struct CompositeRecoveryEvidence {
    pub(super) parent_version: u64,
    pub(super) child_run_ids: Vec<String>,
}

pub(super) async fn exercise_composite_process_death_matrix(
    fixture: &Fixture,
    runtime: &RecoveryRuntime,
    scenario: CompositeScenario,
    committed_index: usize,
    terminal_index: usize,
) -> TestResult<CompositeRecoveryEvidence> {
    let input = scenario.input(&fixture.document);
    let label = scenario.label();
    let created = super::create_workflow_run(
        &fixture.executor,
        fixture.document.actor,
        input,
        super::start_idempotency(input)?,
    )
    .await?;
    if created.replayed
        || created.value.run.status != WorkflowRunStatus::Pending
        || created.value.run.aggregate_version != 1
    {
        return Err(format!("{label} WorkflowRun was not created once as Pending").into());
    }
    let start = runtime.operation_reconciler().run_once().await?;
    if start.inspected != 1 || start.projected != 1 || !start.failures.is_empty() {
        return Err(format!(
            "{label} parent Operation did not start its existing Flow exactly once: {start:#?}"
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
        return Err(format!("{label} parent did not suspend before child dispatch").into());
    }
    let frames = composite_frames(input, &initial_snapshot)?;
    if frames.len() != scenario.child_count() {
        return Err(format!(
            "{label} parent opened {} frames instead of {}",
            frames.len(),
            scenario.child_count()
        )
        .into());
    }

    let committed_marker =
        super::crash_at(fixture, committed_index, scenario.committed_mode()).await?;
    let committed_children = load_children(&runtime.runs, input, &frames).await?;
    require_composite_marker_identity(
        &committed_marker,
        scenario.committed_mode(),
        input,
        &frames,
        &committed_children,
    )?;
    for child in &committed_children {
        if child.run.status != WorkflowRunStatus::Pending || child.run.aggregate_version != 1 {
            return Err(format!(
                "{label} child was not atomically committed once as Pending: {:?}",
                child.run
            )
            .into());
        }
        if runtime
            .operations
            .find_request(child.run.operation_id)
            .await?
            .is_none()
            || runtime
                .operations
                .find_projection(child.run.operation_id)
                .await?
                .is_some()
        {
            return Err(
                format!("{label} child commit crossed its Operation start boundary").into(),
            );
        }
    }
    super::require_run_version(
        &runtime.runs,
        input,
        1,
        &format!("{label} child commit process death"),
    )
    .await?;
    let committed_snapshot = runtime
        .engine
        .snapshot(&input.workflow_run_id.to_string())
        .await?;
    if !committed_snapshot.child_operations.is_empty() {
        return Err(format!("{label} child was linked before its Flow existed").into());
    }

    let stored = runtime
        .runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or_else(|| format!("{label} parent disappeared during child adoption"))?;
    let adopted_projection = runtime
        .coordinator()
        .reconcile(&stored, Utc::now())
        .await?
        .ok_or_else(|| format!("{label} child adoption produced no waiting projection"))?;
    if adopted_projection.run.status != WorkflowRunStatus::Waiting {
        return Err(format!("{label} child adoption changed the suspended parent").into());
    }
    let adopted_children = load_children(&runtime.runs, input, &frames).await?;
    if adopted_children != committed_children {
        return Err(format!("{label} child adoption changed exact child authority").into());
    }
    super::require_run_version(
        &runtime.runs,
        input,
        1,
        &format!("{label} child adoption replay"),
    )
    .await?;

    let child_start = runtime.operation_reconciler().run_once().await?;
    let expected_inspected = scenario.child_count() + 1;
    if child_start.inspected != expected_inspected
        || child_start.projected != scenario.child_count()
        || !child_start.failures.is_empty()
    {
        return Err(format!(
            "{label} child Operations did not start beside one stable parent: {child_start:#?}"
        )
        .into());
    }
    let coordinator = runtime.coordinator();
    let mut terminal_children = Vec::with_capacity(frames.len());
    let mut child_histories = Vec::with_capacity(frames.len());
    for (frame, child) in frames.iter().zip(&committed_children) {
        let operation = runtime
            .operations
            .find_projection(child.run.operation_id)
            .await?
            .ok_or_else(|| format!("{label} child Operation projection disappeared"))?;
        if operation.status
            != a3s_cloud_control_plane::modules::operations::OperationStatus::Succeeded
        {
            return Err(format!(
                "{label} child Operation projected {:?} instead of succeeded",
                operation.status
            )
            .into());
        }
        let history = runtime.engine.history(&child.run.flow_run_id).await?;
        super::require_completed_history(&history)?;
        let stored_child = runtime
            .runs
            .find(input.organization_id, child.run.id)
            .await?
            .ok_or_else(|| format!("{label} child disappeared before terminal projection"))?;
        let expected_version = stored_child.run.aggregate_version;
        let projected = coordinator
            .reconcile(&stored_child, Utc::now())
            .await?
            .ok_or_else(|| format!("{label} child produced no terminal projection"))?;
        let saved = runtime
            .runs
            .save_projection(projected, expected_version)
            .await?;
        if saved.run.status != WorkflowRunStatus::Completed
            || saved.run.aggregate_version != 2
            || saved.run.output.as_ref() != Some(&frame.child_input)
        {
            return Err(
                format!("{label} child terminal projection drifted: {:?}", saved.run).into(),
            );
        }
        terminal_children.push(saved);
        child_histories.push(history);
    }
    super::require_run_version(
        &runtime.runs,
        input,
        1,
        &format!("{label} child terminal projection"),
    )
    .await?;

    let terminal_marker =
        super::crash_at(fixture, terminal_index, scenario.terminal_mode()).await?;
    require_composite_marker_identity(
        &terminal_marker,
        scenario.terminal_mode(),
        input,
        &frames,
        &terminal_children,
    )?;
    super::require_run_version(
        &runtime.runs,
        input,
        1,
        &format!("{label} terminal resume process death"),
    )
    .await?;
    let terminal_snapshot = runtime
        .engine
        .snapshot(&input.workflow_run_id.to_string())
        .await?;
    if terminal_snapshot.status != FlowRunStatus::Completed {
        return Err(format!("{label} terminal children did not complete the parent Flow").into());
    }
    require_composite_child_references(input, &frames, &terminal_children, &terminal_snapshot)?;
    let terminal_history = runtime
        .engine
        .history(&input.workflow_run_id.to_string())
        .await?;
    super::require_completed_history(&terminal_history)?;

    let recovery = runtime.workflow_reconciler()?.run_once(100).await?;
    if recovery.inspected != 1
        || recovery.projected != 1
        || recovery.deferred != 0
        || !recovery.failures.is_empty()
    {
        return Err(format!(
            "{label} terminal parent projection was not recovered exactly once: {recovery:#?}"
        )
        .into());
    }
    let completed = runtime
        .runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or_else(|| format!("{label} terminal parent projection disappeared"))?;
    if completed.run.status != WorkflowRunStatus::Completed
        || completed.run.aggregate_version != 2
        || completed.run.last_flow_sequence != terminal_snapshot.last_sequence
        || completed.run.output.as_ref() != Some(&input.goal_input)
    {
        return Err(format!(
            "{label} recovered parent projection drifted: {:?}",
            completed.run
        )
        .into());
    }
    let parent_operation = runtime.operation_reconciler().run_once().await?;
    if parent_operation.inspected != 1
        || parent_operation.projected != 1
        || !parent_operation.failures.is_empty()
    {
        return Err(format!(
            "{label} parent Operation did not project terminal recovery: {parent_operation:#?}"
        )
        .into());
    }
    let stable_workflow = runtime.workflow_reconciler()?.run_once(100).await?;
    let stable_operation = runtime.operation_reconciler().run_once().await?;
    if stable_workflow.inspected != 0 || stable_operation.inspected != 0 {
        return Err(format!(
            "{label} terminal recovery remained eligible: workflow={stable_workflow:#?}, operation={stable_operation:#?}"
        )
        .into());
    }
    super::require_history_unchanged(
        &runtime.engine,
        input,
        &terminal_history,
        &format!("{label} terminal projection replay"),
    )
    .await?;
    for ((child, history), frame) in terminal_children.iter().zip(&child_histories).zip(&frames) {
        let replayed = runtime.engine.history(&child.run.flow_run_id).await?;
        if &replayed != history {
            return Err(format!(
                "{label} frame {} appended duplicate child Flow history",
                frame.ordinal
            )
            .into());
        }
    }
    Ok(CompositeRecoveryEvidence {
        parent_version: completed.run.aggregate_version,
        child_run_ids: terminal_children
            .iter()
            .map(|child| child.run.id.to_string())
            .collect(),
    })
}

pub(super) async fn coordinate_probe(
    executor: &PostgresExecutor,
    postgres_url: &str,
    input: &WorkflowRunInput,
    mode: ProbeMode,
) -> TestResult<CrashMarker> {
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(ProcessDeathFlowRuntime)).await?;
    let engine = flow.engine();
    let runs: Arc<dyn IWorkflowRunRepository> =
        Arc::new(PostgresWorkflowRunRepository::new(executor.clone()));
    let coordinator = coordinator(engine.clone(), executor, Arc::clone(&runs));
    let stored = runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or("composite probe could not find its parent WorkflowRun")?;
    let before = engine.snapshot(&input.workflow_run_id.to_string()).await?;
    let frames = composite_frames(input, &before)?;
    let projected = coordinator
        .reconcile(&stored, Utc::now())
        .await?
        .ok_or("composite probe did not produce a WorkflowRun projection")?;
    let children = load_children(&runs, input, &frames).await?;
    let snapshot = engine.snapshot(&input.workflow_run_id.to_string()).await?;
    match mode {
        ProbeMode::LoopChildCommitted | ProbeMode::IterationChildrenCommitted => {
            if projected.run.status != WorkflowRunStatus::Waiting
                || snapshot.status != FlowRunStatus::Suspended
                || !snapshot.child_operations.is_empty()
                || children.iter().any(|child| {
                    child.run.status != WorkflowRunStatus::Pending
                        || child.run.aggregate_version != 1
                })
            {
                return Err("composite child-committed boundary drifted".into());
            }
        }
        ProbeMode::LoopTerminalResumed | ProbeMode::IterationTerminalResumed => {
            if projected.run.status != WorkflowRunStatus::Completed
                || snapshot.status != FlowRunStatus::Completed
                || snapshot.child_operations.len() != frames.len()
                || children.iter().any(|child| {
                    child.run.status != WorkflowRunStatus::Completed
                        || child.run.aggregate_version != 2
                })
            {
                return Err("composite terminal-resumed boundary drifted".into());
            }
        }
        _ => return Err("non-composite mode reached the composite probe".into()),
    }
    composite_marker(mode, &projected, &frames, &children)
}

pub(super) fn composite_port(
    executor: &PostgresExecutor,
    runs: Arc<dyn IWorkflowRunRepository>,
) -> Arc<dyn IWorkflowCompositeExecutionPort> {
    let workflows: Arc<dyn IWorkflowDefinitionRepository> =
        Arc::new(PostgresWorkflowDefinitionRepository::new(executor.clone()));
    let ontologies: Arc<dyn IOntologyRepository> =
        Arc::new(PostgresOntologyRepository::new(executor.clone()));
    let goals: Arc<dyn IWorkflowGoalRepository> =
        Arc::new(PostgresWorkflowGoalRepository::new(executor.clone()));
    Arc::new(WorkflowCompositeExecutionApplicationService::new(
        workflows, ontologies, goals, runs,
    ))
}

fn coordinator(
    engine: FlowEngine,
    executor: &PostgresExecutor,
    runs: Arc<dyn IWorkflowRunRepository>,
) -> FlowWorkflowRunCoordinator {
    let executions: Arc<dyn IExecutionRepository> =
        Arc::new(PostgresExecutionRepository::new(executor.clone()));
    let templates: Arc<dyn IExecutionTemplateRepository> =
        Arc::new(PostgresExecutionTemplateRepository::new(executor.clone()));
    let execution_port: Arc<dyn IWorkflowExecutionPort> =
        Arc::new(WorkflowExecutionApplicationService::new(
            Arc::new(PostgresProjectsRepository::new(executor.clone())),
            templates,
            executions,
        ));
    let composite_port = composite_port(executor, runs);
    FlowWorkflowRunCoordinator::with_ports(engine, execution_port, composite_port)
}

fn composite_frames(
    input: &WorkflowRunInput,
    snapshot: &WorkflowRunSnapshot,
) -> TestResult<Vec<WorkflowCompositeFrame>> {
    let resolved_variables = input
        .variable_contract
        .as_ref()
        .ok_or("composite process-death input lost its variable contract")?;
    let variables = WorkflowVariableContract::restore(
        &resolved_variables.canonical_acl,
        resolved_variables.digest.as_str(),
    )?;
    let resolved_regions = input
        .composite_regions
        .as_ref()
        .ok_or("composite process-death input lost its region contract")?;
    let regions = WorkflowCompositeRegions::restore(
        &resolved_regions.canonical_acl,
        resolved_regions.digest.as_str(),
    )?;
    let defaults = input
        .variable_defaults
        .as_ref()
        .map(|resolved| {
            WorkflowVariableDefaults::restore(&resolved.canonical_acl, resolved.digest.as_str())
        })
        .transpose()?;
    let mut frames = Vec::new();
    for hook in snapshot.hooks.values() {
        if hook.hook_id.starts_with("workflow-composite-wave:") {
            let metadata: WorkflowCompositeWaveHookMetadata =
                serde_json::from_value(hook.metadata.clone())?;
            frames.extend(metadata.frames(&input.plan, &regions, &variables, defaults.as_ref())?);
        } else if hook.hook_id.starts_with("workflow-composite:") {
            let metadata: WorkflowCompositeHookMetadata =
                serde_json::from_value(hook.metadata.clone())?;
            metadata.validate(&input.plan, &regions, &variables)?;
            frames.push(metadata.frame);
        }
    }
    frames.sort_by_key(|frame| frame.ordinal);
    if frames.is_empty() {
        return Err("composite process-death Flow exposed no frame".into());
    }
    Ok(frames)
}

async fn load_children(
    runs: &Arc<dyn IWorkflowRunRepository>,
    input: &WorkflowRunInput,
    frames: &[WorkflowCompositeFrame],
) -> TestResult<Vec<WorkflowRunRecord>> {
    let mut children = Vec::with_capacity(frames.len());
    for frame in frames {
        children.push(
            runs.find(input.organization_id, frame.child_workflow_run_id())
                .await?
                .ok_or_else(|| {
                    format!(
                        "composite process-death frame {} lost its exact child",
                        frame.ordinal
                    )
                })?,
        );
    }
    Ok(children)
}

fn composite_marker(
    mode: ProbeMode,
    parent: &WorkflowRunRecord,
    frames: &[WorkflowCompositeFrame],
    children: &[WorkflowRunRecord],
) -> TestResult<CrashMarker> {
    if frames.len() != children.len() {
        return Err("composite marker frame and child counts differ".into());
    }
    let mut marker = super::workflow_marker(mode, parent);
    marker.composite_children = Some(
        frames
            .iter()
            .zip(children)
            .map(|(frame, child)| CompositeChildMarker {
                ordinal: frame.ordinal,
                reference_id: frame.child_reference_id(),
                frame_digest: frame.frame_digest.to_string(),
                workflow_run_id: child.run.id.to_string(),
                operation_id: child.run.operation_id.to_string(),
                status: child.run.status.as_str().into(),
                aggregate_version: child.run.aggregate_version,
                workflow_goal_id: child.run.workflow_goal_id.to_string(),
                plan_revision_id: child.run.plan_revision_id.to_string(),
                plan_digest: child.run.plan_digest.to_string(),
            })
            .collect(),
    );
    Ok(marker)
}

fn require_composite_marker_identity(
    marker: &serde_json::Value,
    mode: ProbeMode,
    input: &WorkflowRunInput,
    frames: &[WorkflowCompositeFrame],
    children: &[WorkflowRunRecord],
) -> TestResult {
    super::require_marker_identity(marker, mode, input)?;
    let observed = marker["compositeChildren"]
        .as_array()
        .ok_or("composite crash marker omitted its children")?;
    if observed.len() != frames.len() || frames.len() != children.len() {
        return Err("composite crash marker child count drifted".into());
    }
    for ((value, frame), child) in observed.iter().zip(frames).zip(children) {
        if value["ordinal"].as_u64() != Some(u64::from(frame.ordinal))
            || value["referenceId"].as_str() != Some(frame.child_reference_id().as_str())
            || value["frameDigest"].as_str() != Some(frame.frame_digest.as_str())
            || value["workflowRunId"].as_str() != Some(child.run.id.to_string().as_str())
            || value["operationId"].as_str() != Some(child.run.operation_id.to_string().as_str())
            || value["status"].as_str() != Some(child.run.status.as_str())
            || value["aggregateVersion"].as_u64() != Some(child.run.aggregate_version)
            || value["workflowGoalId"].as_str()
                != Some(child.run.workflow_goal_id.to_string().as_str())
            || value["planRevisionId"].as_str()
                != Some(child.run.plan_revision_id.to_string().as_str())
            || value["planDigest"].as_str() != Some(child.run.plan_digest.as_str())
        {
            return Err(format!(
                "composite crash marker frame {} drifted: {value}",
                frame.ordinal
            )
            .into());
        }
    }
    Ok(())
}

fn require_composite_child_references(
    input: &WorkflowRunInput,
    frames: &[WorkflowCompositeFrame],
    children: &[WorkflowRunRecord],
    snapshot: &WorkflowRunSnapshot,
) -> TestResult {
    if snapshot.child_operations.len() != frames.len() || frames.len() != children.len() {
        return Err("composite parent retained an unexpected child reference count".into());
    }
    for (frame, child) in frames.iter().zip(children) {
        let reference_id = frame.child_reference_id();
        let reference = snapshot
            .child_operations
            .get(&reference_id)
            .ok_or_else(|| format!("composite parent lost child reference {reference_id}"))?;
        if reference.reference_id != reference_id
            || reference.kind != "workflow_run"
            || reference.operation_id != child.run.operation_id.to_string()
            || reference.flow_run_id.as_deref() != Some(child.run.flow_run_id.as_str())
        {
            return Err(format!(
                "composite child reference {} identity drifted",
                frame.ordinal
            )
            .into());
        }
        let metadata: WorkflowCompositeChildReferenceMetadata =
            serde_json::from_value(reference.metadata.clone())?;
        metadata.validate_frame(frame)?;
        if metadata.schema != WORKFLOW_COMPOSITE_CHILD_REFERENCE_SCHEMA
            || metadata.child_workflow_run_id != child.run.id
            || metadata.child_workflow_goal_id != child.run.workflow_goal_id
            || metadata.child_operation_id != child.run.operation_id
            || metadata.child_plan_revision_id != child.run.plan_revision_id
            || metadata.child_plan_digest != child.run.plan_digest
            || metadata.parent_workflow_run_id != input.workflow_run_id
        {
            return Err(format!(
                "composite child reference {} metadata drifted",
                frame.ordinal
            )
            .into());
        }
    }
    Ok(())
}

pub(super) fn input_for_mode(
    document: &ProbeDocument,
    mode: ProbeMode,
) -> Option<&WorkflowRunInput> {
    match mode {
        ProbeMode::LoopChildCommitted | ProbeMode::LoopTerminalResumed => {
            Some(&document.loop_input)
        }
        ProbeMode::IterationChildrenCommitted | ProbeMode::IterationTerminalResumed => {
            Some(&document.iteration_input)
        }
        _ => None,
    }
}
