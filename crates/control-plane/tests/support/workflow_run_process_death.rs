#[path = "workflow_run_process_death_composite.rs"]
mod composite;
#[path = "workflow_run_process_death_composite_fixture.rs"]
mod composite_fixture;
#[path = "workflow_run_process_death_evidence.rs"]
mod evidence;
#[path = "workflow_run_process_death_execution.rs"]
mod execution;
#[path = "workflow_run_process_death_fixture.rs"]
mod fixture;
#[path = "workflow_run_process_death_process.rs"]
mod process;

use a3s_cloud_control_plane::infrastructure::FlowInfrastructure;
use a3s_cloud_control_plane::modules::executions::{
    Execution, ExecutionReconciler, ExecutionStatus, IExecutionRepository,
    IExecutionTemplateRepository, IWorkflowExecutionPort, PostgresExecutionRepository,
    PostgresExecutionTemplateRepository, WorkflowExecutionApplicationService,
    EXECUTION_WORKFLOW_NAME, EXECUTION_WORKFLOW_VERSION,
};
use a3s_cloud_control_plane::modules::operations::{
    FlowOperationEngine, IOperationEngine, IOperationRepository, OperationReconciler,
    OperationStatus, PostgresOperationRepository, ReconcileOperationsHandler,
};
use a3s_cloud_control_plane::modules::projects::PostgresProjectsRepository;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{IdempotencyRequest, OperationId};
use a3s_cloud_control_plane::modules::workflow::{
    CancelWorkflowRunWrite, CreateWorkflowRunWrite, FlowWorkflowRunCoordinator,
    IWorkflowCompositeExecutionPort, IWorkflowRunCoordinator, IWorkflowRunRepository,
    PostgresWorkflowRunRepository, WorkflowRun, WorkflowRunCancellationRequested,
    WorkflowRunFlowRuntime, WorkflowRunInput, WorkflowRunReconciler, WorkflowRunRecord,
    WorkflowRunRequested, WorkflowRunStatus,
};
use a3s_flow::{
    FlowError, FlowRuntime, RuntimeCommand, StepInvocation, WorkflowInvocation,
    WorkflowRunStatus as FlowRunStatus,
};
use a3s_orm::PostgresExecutor;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use evidence::*;
use fixture::{Fixture, ProbeDocument, EXECUTION_STEP_ID};
use process::{crash_at, probe_environment, publish_marker, CrashMarker, ProbeMode};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CANCELLATION_REASON: &str = "operator cancelled the process-death probe";

#[derive(Debug, Clone, Copy)]
struct ProcessDeathFlowRuntime;

#[async_trait::async_trait]
impl FlowRuntime for ProcessDeathFlowRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> Result<RuntimeCommand, FlowError> {
        if invocation.spec.name == EXECUTION_WORKFLOW_NAME
            && invocation.spec.version == EXECUTION_WORKFLOW_VERSION
        {
            return Ok(invocation.context().complete(serde_json::json!({
                "processDeathProbe": true
            })));
        }
        WorkflowRunFlowRuntime::default()
            .run_workflow(invocation)
            .await
    }

    async fn run_step(&self, invocation: StepInvocation) -> Result<serde_json::Value, FlowError> {
        WorkflowRunFlowRuntime::default().run_step(invocation).await
    }
}

pub async fn exercise_process_death_matrix(postgres_url: String) -> TestResult {
    let state = tempfile::tempdir()?;
    let fixture = fixture::setup_fixture(postgres_url, state.path()).await?;
    let terminal = &fixture.document.terminal_input;

    let create_marker = crash_at(&fixture, 1, ProbeMode::CreateCommit).await?;
    require_marker_identity(&create_marker, ProbeMode::CreateCommit, terminal)?;
    let replayed = create_workflow_run(
        &fixture.executor,
        fixture.document.actor,
        terminal,
        start_idempotency(terminal)?,
    )
    .await?;
    if !replayed.replayed {
        return Err("WorkflowRun start retry did not replay the committed API result".into());
    }
    require_run_identity(&replayed.value, terminal)?;
    if replayed.value.run.status != WorkflowRunStatus::Pending
        || replayed.value.run.aggregate_version != 1
    {
        return Err("replayed WorkflowRun start changed the committed pending aggregate".into());
    }
    let operations: Arc<dyn IOperationRepository> =
        Arc::new(PostgresOperationRepository::new(fixture.executor.clone()));
    let request = operations
        .find_request(replayed.value.run.operation_id)
        .await?
        .ok_or("WorkflowRun start did not atomically commit its Operation request")?;
    if request.id != replayed.value.run.operation_id
        || request.subject.id() != replayed.value.run.id.as_uuid()
        || request.input != serde_json::to_value(terminal)?
    {
        return Err("WorkflowRun start replay drifted from its correlated Operation".into());
    }
    if operations
        .find_projection(replayed.value.run.operation_id)
        .await?
        .is_some()
    {
        return Err("Operation projection appeared before the worker started Flow".into());
    }

    let flow_marker = crash_at(&fixture, 2, ProbeMode::FlowStarted).await?;
    require_marker_identity(&flow_marker, ProbeMode::FlowStarted, terminal)?;
    let runtime = RecoveryRuntime::connect(&fixture).await?;
    let terminal_history_before = runtime
        .engine
        .history(&terminal.workflow_run_id.to_string())
        .await?;
    require_completed_history(&terminal_history_before)?;
    if runtime
        .operations
        .find_projection(replayed.value.run.operation_id)
        .await?
        .is_some()
    {
        return Err("Operation projection committed before the killed worker returned".into());
    }
    let operation_report = runtime.operation_reconciler().run_once().await?;
    if operation_report.inspected != 1
        || operation_report.projected != 1
        || !operation_report.failures.is_empty()
    {
        return Err(format!(
            "restarted Operation reconciler did not adopt the completed Flow exactly once: {operation_report:#?}"
        )
        .into());
    }
    let operation = runtime
        .operations
        .find_projection(replayed.value.run.operation_id)
        .await?
        .ok_or("restarted Operation reconciler did not persist a projection")?;
    if operation.status != OperationStatus::Succeeded {
        return Err(format!(
            "restarted Operation projected {:?} instead of succeeded",
            operation.status
        )
        .into());
    }
    require_history_unchanged(
        &runtime.engine,
        terminal,
        &terminal_history_before,
        "Operation recovery",
    )
    .await?;

    let observed_marker = crash_at(&fixture, 3, ProbeMode::TerminalObserved).await?;
    require_marker_identity(&observed_marker, ProbeMode::TerminalObserved, terminal)?;
    let before_projection = runtime
        .runs
        .find(terminal.organization_id, terminal.workflow_run_id)
        .await?
        .ok_or("WorkflowRun disappeared before terminal projection recovery")?;
    if before_projection.run.status != WorkflowRunStatus::Pending
        || before_projection.run.aggregate_version != 1
        || before_projection.run.last_flow_sequence != 0
    {
        return Err(
            "killed worker committed the WorkflowRun projection before process death".into(),
        );
    }
    let workflow_report = runtime.workflow_reconciler()?.run_once(100).await?;
    if workflow_report.inspected != 1
        || workflow_report.projected != 1
        || workflow_report.deferred != 0
        || !workflow_report.failures.is_empty()
    {
        return Err(format!(
            "restarted WorkflowRun reconciler did not project the terminal Flow exactly once: {workflow_report:#?}"
        )
        .into());
    }
    let completed = runtime
        .runs
        .find(terminal.organization_id, terminal.workflow_run_id)
        .await?
        .ok_or("terminal WorkflowRun projection was not persisted")?;
    if completed.run.status != WorkflowRunStatus::Completed
        || completed.run.output.as_ref() != Some(&terminal.goal_input)
        || completed.run.aggregate_version != 2
        || completed.run.last_flow_sequence != operation.last_sequence
    {
        return Err(format!(
            "recovered terminal WorkflowRun projection drifted: {:?}",
            completed.run
        )
        .into());
    }
    let stable_terminal_version = completed.run.aggregate_version;
    let replay_report = runtime.workflow_reconciler()?.run_once(100).await?;
    if replay_report.inspected != 0 {
        return Err(format!(
            "terminal WorkflowRun remained eligible for projection replay: {replay_report:#?}"
        )
        .into());
    }
    require_run_version(
        &runtime.runs,
        terminal,
        stable_terminal_version,
        "terminal projection replay",
    )
    .await?;
    require_history_unchanged(
        &runtime.engine,
        terminal,
        &terminal_history_before,
        "WorkflowRun projection recovery",
    )
    .await?;

    let cancellation = &fixture.document.cancellation_input;
    let cancellation_created = create_workflow_run(
        &fixture.executor,
        fixture.document.actor,
        cancellation,
        start_idempotency(cancellation)?,
    )
    .await?;
    if cancellation_created.replayed {
        return Err("cancellation WorkflowRun fixture unexpectedly replayed its creation".into());
    }
    let cancellation_start = runtime.operation_reconciler().run_once().await?;
    if cancellation_start.inspected != 1
        || cancellation_start.projected != 1
        || !cancellation_start.failures.is_empty()
    {
        return Err(format!(
            "cancellation WorkflowRun did not start and suspend exactly once: {cancellation_start:#?}"
        )
        .into());
    }
    let cancellation_snapshot = runtime
        .engine
        .snapshot(&cancellation.workflow_run_id.to_string())
        .await?;
    if cancellation_snapshot.status != FlowRunStatus::Suspended {
        return Err(format!(
            "cancellation WorkflowRun Flow reached {:?} instead of suspended",
            cancellation_snapshot.status
        )
        .into());
    }
    let waiting_report = runtime.workflow_reconciler()?.run_once(100).await?;
    if waiting_report.inspected != 1
        || waiting_report.projected != 1
        || !waiting_report.failures.is_empty()
    {
        return Err(format!(
            "cancellation WorkflowRun did not project Waiting exactly once: {waiting_report:#?}"
        )
        .into());
    }
    let waiting = runtime
        .runs
        .find(cancellation.organization_id, cancellation.workflow_run_id)
        .await?
        .ok_or("waiting WorkflowRun projection was not persisted")?;
    if waiting.run.status != WorkflowRunStatus::Waiting || waiting.run.aggregate_version != 2 {
        return Err("cancellation WorkflowRun did not enter Waiting before cancellation".into());
    }

    let cancellation_history_before = runtime
        .engine
        .history(&cancellation.workflow_run_id.to_string())
        .await?;
    let cancellation_marker = crash_at(&fixture, 4, ProbeMode::CancellationCommit).await?;
    require_marker_identity(
        &cancellation_marker,
        ProbeMode::CancellationCommit,
        cancellation,
    )?;
    let cancelling = runtime
        .runs
        .find(cancellation.organization_id, cancellation.workflow_run_id)
        .await?
        .ok_or("committed WorkflowRun cancellation disappeared after process death")?;
    if cancelling.run.status != WorkflowRunStatus::Cancelling
        || cancelling.run.aggregate_version != waiting.run.aggregate_version + 1
        || cancelling.run.cancellation_reason.as_deref() != Some(CANCELLATION_REASON)
    {
        return Err("WorkflowRun cancellation transaction was not durably committed once".into());
    }
    let cancellation_replay = runtime
        .runs
        .replay(&cancellation_idempotency(cancellation)?)
        .await?
        .ok_or("WorkflowRun cancellation idempotency record was not committed")?;
    if cancellation_replay.run != cancelling.run {
        return Err(
            "WorkflowRun cancellation replay did not return the committed aggregate".into(),
        );
    }
    require_history_unchanged(
        &runtime.engine,
        cancellation,
        &cancellation_history_before,
        "cancellation transaction",
    )
    .await?;

    let cancelled_report = runtime.workflow_reconciler()?.run_once(100).await?;
    if cancelled_report.inspected != 1
        || cancelled_report.projected != 1
        || cancelled_report.deferred != 0
        || !cancelled_report.failures.is_empty()
    {
        return Err(format!(
            "restarted WorkflowRun reconciler did not deliver cancellation exactly once: {cancelled_report:#?}"
        )
        .into());
    }
    let cancelled = runtime
        .runs
        .find(cancellation.organization_id, cancellation.workflow_run_id)
        .await?
        .ok_or("cancelled WorkflowRun projection was not persisted")?;
    if cancelled.run.status != WorkflowRunStatus::Cancelled
        || cancelled.run.aggregate_version != waiting.run.aggregate_version + 2
    {
        return Err(format!(
            "recovered cancellation projection drifted: {:?}",
            cancelled.run
        )
        .into());
    }
    let cancellation_history = runtime
        .engine
        .history(&cancellation.workflow_run_id.to_string())
        .await?;
    require_cancellation_history(&cancellation_history)?;
    let cancellation_operation = runtime.operation_reconciler().run_once().await?;
    if cancellation_operation.inspected != 1
        || cancellation_operation.projected != 1
        || !cancellation_operation.failures.is_empty()
    {
        return Err(format!(
            "restarted Operation reconciler did not project cancellation once: {cancellation_operation:#?}"
        )
        .into());
    }
    let operation = runtime
        .operations
        .find_projection(cancelled.run.operation_id)
        .await?
        .ok_or("cancelled Operation projection was not persisted")?;
    if operation.status != OperationStatus::Cancelled {
        return Err(format!(
            "cancelled WorkflowRun Operation projected {:?}",
            operation.status
        )
        .into());
    }

    let stable_cancelled_version = cancelled.run.aggregate_version;
    let stable_cancellation_history = cancellation_history.len();
    let final_workflow_report = runtime.workflow_reconciler()?.run_once(100).await?;
    let final_operation_report = runtime.operation_reconciler().run_once().await?;
    if final_workflow_report.inspected != 0 || final_operation_report.inspected != 0 {
        return Err(format!(
            "terminal recovery remained eligible for reconciliation: workflow={final_workflow_report:#?}, operation={final_operation_report:#?}"
        )
        .into());
    }
    require_run_version(
        &runtime.runs,
        cancellation,
        stable_cancelled_version,
        "cancellation replay",
    )
    .await?;
    if runtime
        .engine
        .history(&cancellation.workflow_run_id.to_string())
        .await?
        .len()
        != stable_cancellation_history
    {
        return Err("cancellation recovery appended duplicate Flow history".into());
    }
    let stable_execution_version =
        execution::exercise_execution_child_matrix(&fixture, &runtime).await?;
    let loop_evidence = composite::exercise_composite_process_death_matrix(
        &fixture,
        &runtime,
        composite::CompositeScenario::Loop,
        8,
        9,
    )
    .await?;
    let iteration_evidence = composite::exercise_composite_process_death_matrix(
        &fixture,
        &runtime,
        composite::CompositeScenario::Iteration,
        10,
        11,
    )
    .await?;
    let composite_child_run_ids = loop_evidence
        .child_run_ids
        .iter()
        .chain(&iteration_evidence.child_run_ids)
        .cloned()
        .collect::<Vec<_>>();
    verify_database_evidence(&fixture, &composite_child_run_ids).await?;
    let run_ids = runtime.engine.list_run_ids().await?;
    let execution = &fixture.document.execution_input;
    let child = runtime
        .executions
        .find_for_workflow(
            execution.organization_id,
            execution.workflow_run_id,
            EXECUTION_STEP_ID,
            1,
        )
        .await?
        .ok_or("finite child Execution disappeared from final evidence")?;
    if run_ids.len() != 9
        || !run_ids.contains(&terminal.workflow_run_id.to_string())
        || !run_ids.contains(&cancellation.workflow_run_id.to_string())
        || !run_ids.contains(&execution.workflow_run_id.to_string())
        || !run_ids.contains(&child.operation_id.to_string())
        || !run_ids.contains(&fixture.document.loop_input.workflow_run_id.to_string())
        || !run_ids.contains(&fixture.document.iteration_input.workflow_run_id.to_string())
        || composite_child_run_ids
            .iter()
            .any(|child_run_id| !run_ids.contains(child_run_id))
    {
        return Err(format!(
            "process-death recovery did not preserve the five parent and four child Flow runs: {run_ids:?}"
        )
        .into());
    }

    println!(
        "A3S_CLOUD_WORKFLOW_RUN_PROCESS_DEATH_CERTIFIED boundaries=11 sigkills=11 workflow_runs=8 executions=1 operations=9 flow_runs=9 terminal_version={stable_terminal_version} cancellation_version={stable_cancelled_version} execution_version={stable_execution_version} loop_version={} iteration_version={}",
        loop_evidence.parent_version,
        iteration_evidence.parent_version,
    );
    Ok(())
}

pub async fn run_probe() -> TestResult {
    let environment = probe_environment()?;
    let postgres_url = environment.postgres_url;
    let state_dir = environment.state_dir;
    let mode = environment.mode;
    let marker = environment.marker;
    let document = fixture::load_document(&state_dir)?;
    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 4)?;

    match mode {
        ProbeMode::CreateCommit => {
            let write = create_workflow_run(
                &executor,
                document.actor,
                &document.terminal_input,
                start_idempotency(&document.terminal_input)?,
            )
            .await?;
            if write.replayed {
                return Err("create-commit probe unexpectedly replayed WorkflowRun start".into());
            }
            publish_marker(&marker, workflow_marker(mode, &write.value))?;
        }
        ProbeMode::FlowStarted => {
            let flow =
                FlowInfrastructure::connect(&postgres_url, Arc::new(ProcessDeathFlowRuntime))
                    .await?;
            let operations = PostgresOperationRepository::new(executor);
            let operation_id =
                OperationId::from_uuid(document.terminal_input.workflow_run_id.as_uuid());
            let request = operations
                .find_request(operation_id)
                .await?
                .ok_or("flow-started probe could not find the Operation request")?;
            let projection = FlowOperationEngine::new(flow.engine())
                .ensure(&request)
                .await?;
            if projection.status != OperationStatus::Succeeded {
                return Err(format!(
                    "flow-started probe observed {:?} instead of succeeded",
                    projection.status
                )
                .into());
            }
            publish_marker(
                &marker,
                CrashMarker {
                    mode: mode.as_str().into(),
                    workflow_run_id: document.terminal_input.workflow_run_id.to_string(),
                    operation_id: operation_id.to_string(),
                    flow_run_id: document.terminal_input.workflow_run_id.to_string(),
                    status: projection.status.as_str().into(),
                    aggregate_version: 1,
                    last_flow_sequence: projection.last_sequence,
                    execution_id: None,
                    execution_operation_id: None,
                    execution_status: None,
                    execution_aggregate_version: None,
                    execution_template_id: None,
                    execution_template_revision_id: None,
                    execution_template_digest: None,
                    invocation_template_digest: None,
                    composite_children: None,
                },
            )?;
        }
        ProbeMode::TerminalObserved => {
            let flow =
                FlowInfrastructure::connect(&postgres_url, Arc::new(ProcessDeathFlowRuntime))
                    .await?;
            let runs = PostgresWorkflowRunRepository::new(executor);
            let stored = runs
                .find(
                    document.terminal_input.organization_id,
                    document.terminal_input.workflow_run_id,
                )
                .await?
                .ok_or("terminal-observed probe could not find the WorkflowRun")?;
            let expected_version = stored.run.aggregate_version;
            let projected = FlowWorkflowRunCoordinator::new(flow.engine())
                .reconcile(&stored, Utc::now())
                .await?
                .ok_or("terminal-observed probe did not produce a projection")?;
            if projected.run.aggregate_version != expected_version + 1
                || projected.run.status != WorkflowRunStatus::Completed
            {
                return Err("terminal-observed probe produced an invalid projection".into());
            }
            publish_marker(&marker, workflow_marker(mode, &projected))?;
        }
        ProbeMode::CancellationCommit => {
            let write = request_cancellation(&executor, &document).await?;
            if write.replayed {
                return Err("cancellation-commit probe unexpectedly replayed cancellation".into());
            }
            publish_marker(&marker, workflow_marker(mode, &write.value))?;
        }
        ProbeMode::ExecutionChildCommitted
        | ProbeMode::ExecutionChildLinked
        | ProbeMode::ExecutionTerminalResumed => {
            let (engine, projected, execution) =
                coordinate_execution_probe(&executor, &postgres_url, &document.execution_input)
                    .await?;
            let snapshot = engine
                .snapshot(&document.execution_input.workflow_run_id.to_string())
                .await?;
            match mode {
                ProbeMode::ExecutionChildCommitted => {
                    if projected.run.status != WorkflowRunStatus::Waiting
                        || execution.status != ExecutionStatus::Queued
                        || !snapshot.child_operations.is_empty()
                        || snapshot.status != FlowRunStatus::Suspended
                    {
                        return Err(
                            "child-committed boundary drifted before child Operation start".into(),
                        );
                    }
                }
                ProbeMode::ExecutionChildLinked => {
                    if projected.run.status != WorkflowRunStatus::Waiting
                        || execution.status != ExecutionStatus::Queued
                        || snapshot.child_operations.len() != 1
                        || snapshot.status != FlowRunStatus::Suspended
                    {
                        return Err(
                            "child-linked boundary did not retain one suspended child".into()
                        );
                    }
                }
                ProbeMode::ExecutionTerminalResumed => {
                    if projected.run.status != WorkflowRunStatus::Completed
                        || execution.status != ExecutionStatus::Succeeded
                        || snapshot.child_operations.len() != 1
                        || snapshot.status != FlowRunStatus::Completed
                    {
                        return Err(
                            "terminal-resumed boundary did not complete through its exact child"
                                .into(),
                        );
                    }
                }
                _ => unreachable!("execution probe group only contains Execution modes"),
            }
            publish_marker(&marker, execution_marker(mode, &projected, &execution)?)?;
        }
        ProbeMode::LoopChildCommitted
        | ProbeMode::LoopTerminalResumed
        | ProbeMode::IterationChildrenCommitted
        | ProbeMode::IterationTerminalResumed => {
            let input = composite::input_for_mode(&document, mode)
                .ok_or("composite probe mode lost its WorkflowRun input")?;
            let value = composite::coordinate_probe(&executor, &postgres_url, input, mode).await?;
            publish_marker(&marker, value)?;
        }
    }
    std::future::pending::<()>().await;
    Err("WorkflowRun crash probe resumed after publishing its marker".into())
}

fn workflow_marker(mode: ProbeMode, record: &WorkflowRunRecord) -> CrashMarker {
    CrashMarker {
        mode: mode.as_str().into(),
        workflow_run_id: record.run.id.to_string(),
        operation_id: record.run.operation_id.to_string(),
        flow_run_id: record.run.flow_run_id.clone(),
        status: record.run.status.as_str().into(),
        aggregate_version: record.run.aggregate_version,
        last_flow_sequence: record.run.last_flow_sequence,
        execution_id: None,
        execution_operation_id: None,
        execution_status: None,
        execution_aggregate_version: None,
        execution_template_id: None,
        execution_template_revision_id: None,
        execution_template_digest: None,
        invocation_template_digest: None,
        composite_children: None,
    }
}

fn execution_marker(
    mode: ProbeMode,
    record: &WorkflowRunRecord,
    execution: &Execution,
) -> TestResult<CrashMarker> {
    let binding = execution
        .workflow
        .as_ref()
        .ok_or("process-death child Execution lost its Workflow authority")?;
    let mut marker = workflow_marker(mode, record);
    marker.execution_id = Some(execution.id.to_string());
    marker.execution_operation_id = Some(execution.operation_id.to_string());
    marker.execution_status = Some(execution.status.as_str().into());
    marker.execution_aggregate_version = Some(execution.aggregate_version);
    marker.execution_template_id = Some(binding.execution_template_id.to_string());
    marker.execution_template_revision_id =
        Some(binding.execution_template_revision_id.to_string());
    marker.execution_template_digest = Some(binding.execution_template_digest.to_string());
    marker.invocation_template_digest = Some(execution.template_digest.clone());
    Ok(marker)
}

async fn coordinate_execution_probe(
    executor: &PostgresExecutor,
    postgres_url: &str,
    input: &WorkflowRunInput,
) -> TestResult<(a3s_flow::FlowEngine, WorkflowRunRecord, Execution)> {
    let flow = FlowInfrastructure::connect(postgres_url, Arc::new(ProcessDeathFlowRuntime)).await?;
    let engine = flow.engine();
    let runs: Arc<dyn IWorkflowRunRepository> =
        Arc::new(PostgresWorkflowRunRepository::new(executor.clone()));
    let executions: Arc<dyn IExecutionRepository> =
        Arc::new(PostgresExecutionRepository::new(executor.clone()));
    let templates: Arc<dyn IExecutionTemplateRepository> =
        Arc::new(PostgresExecutionTemplateRepository::new(executor.clone()));
    let execution_port: Arc<dyn IWorkflowExecutionPort> =
        Arc::new(WorkflowExecutionApplicationService::new(
            Arc::new(PostgresProjectsRepository::new(executor.clone())),
            templates,
            Arc::clone(&executions),
        ));
    let stored = runs
        .find(input.organization_id, input.workflow_run_id)
        .await?
        .ok_or("execution probe could not find its WorkflowRun")?;
    let projected = FlowWorkflowRunCoordinator::with_executions(engine.clone(), execution_port)
        .reconcile(&stored, Utc::now())
        .await?
        .ok_or("execution probe did not produce a WorkflowRun projection")?;
    let execution = executions
        .find_for_workflow(
            input.organization_id,
            input.workflow_run_id,
            EXECUTION_STEP_ID,
            1,
        )
        .await?
        .ok_or("execution probe did not create or adopt its exact child")?;
    Ok((engine, projected, execution))
}

struct RecoveryRuntime {
    engine: a3s_flow::FlowEngine,
    operations: Arc<dyn IOperationRepository>,
    runs: Arc<dyn IWorkflowRunRepository>,
    executions: Arc<dyn IExecutionRepository>,
    execution_port: Arc<dyn IWorkflowExecutionPort>,
    composite_port: Arc<dyn IWorkflowCompositeExecutionPort>,
}

impl RecoveryRuntime {
    async fn connect(fixture: &Fixture) -> TestResult<Self> {
        let flow =
            FlowInfrastructure::connect(&fixture.postgres_url, Arc::new(ProcessDeathFlowRuntime))
                .await?;
        let engine = flow.engine();
        let executions: Arc<dyn IExecutionRepository> =
            Arc::new(PostgresExecutionRepository::new(fixture.executor.clone()));
        let templates: Arc<dyn IExecutionTemplateRepository> = Arc::new(
            PostgresExecutionTemplateRepository::new(fixture.executor.clone()),
        );
        let execution_port: Arc<dyn IWorkflowExecutionPort> =
            Arc::new(WorkflowExecutionApplicationService::new(
                Arc::new(PostgresProjectsRepository::new(fixture.executor.clone())),
                templates,
                Arc::clone(&executions),
            ));
        let runs: Arc<dyn IWorkflowRunRepository> =
            Arc::new(PostgresWorkflowRunRepository::new(fixture.executor.clone()));
        let composite_port = composite::composite_port(&fixture.executor, Arc::clone(&runs));
        Ok(Self {
            engine,
            operations: Arc::new(PostgresOperationRepository::new(fixture.executor.clone())),
            runs,
            executions,
            execution_port,
            composite_port,
        })
    }

    fn operation_reconciler(&self) -> OperationReconciler {
        OperationReconciler::new(
            Arc::new(ReconcileOperationsHandler::new(
                Arc::clone(&self.operations),
                Arc::new(FlowOperationEngine::new(self.engine.clone())),
            )),
            100,
        )
    }

    fn workflow_reconciler(&self) -> Result<WorkflowRunReconciler, String> {
        WorkflowRunReconciler::new(
            Arc::clone(&self.runs),
            Arc::new(FlowWorkflowRunCoordinator::with_ports(
                self.engine.clone(),
                Arc::clone(&self.execution_port),
                Arc::clone(&self.composite_port),
            )),
            Duration::from_millis(5),
            100,
        )
    }

    fn execution_reconciler(&self) -> ExecutionReconciler {
        ExecutionReconciler::new(Arc::clone(&self.executions), Arc::clone(&self.operations))
    }

    fn coordinator(&self) -> FlowWorkflowRunCoordinator {
        FlowWorkflowRunCoordinator::with_ports(
            self.engine.clone(),
            Arc::clone(&self.execution_port),
            Arc::clone(&self.composite_port),
        )
    }
}

async fn create_workflow_run(
    executor: &PostgresExecutor,
    actor: a3s_cloud_control_plane::modules::shared_kernel::domain::PrincipalId,
    input: &WorkflowRunInput,
    idempotency: IdempotencyRequest,
) -> TestResult<
    a3s_cloud_control_plane::modules::shared_kernel::domain::IdempotentWrite<WorkflowRunRecord>,
> {
    let (run, steps) = WorkflowRun::create(input.clone(), actor)?;
    let record = WorkflowRunRecord { run, steps };
    let request_id = Uuid::new_v5(&record.run.id.as_uuid(), b"workflow-run-start-request");
    let event = WorkflowRunRequested::envelope(&record.run, request_id)?;
    Ok(PostgresWorkflowRunRepository::new(executor.clone())
        .create(CreateWorkflowRunWrite {
            record,
            event,
            actor_principal_id: actor,
            request_id,
            idempotency,
        })
        .await?)
}

async fn request_cancellation(
    executor: &PostgresExecutor,
    document: &ProbeDocument,
) -> TestResult<
    a3s_cloud_control_plane::modules::shared_kernel::domain::IdempotentWrite<WorkflowRunRecord>,
> {
    let repository = PostgresWorkflowRunRepository::new(executor.clone());
    let mut record = repository
        .find(
            document.cancellation_input.organization_id,
            document.cancellation_input.workflow_run_id,
        )
        .await?
        .ok_or("cancellation-commit probe could not find the WorkflowRun")?;
    let expected_version = record.run.aggregate_version;
    record.run.request_cancellation(
        Some(CANCELLATION_REASON.into()),
        document.actor,
        Utc::now(),
    )?;
    let request_id = Uuid::new_v5(
        &record.run.id.as_uuid(),
        b"workflow-run-cancellation-request",
    );
    let event = WorkflowRunCancellationRequested::envelope(&record.run, request_id)?;
    Ok(repository
        .request_cancellation(CancelWorkflowRunWrite {
            record,
            expected_version,
            event,
            actor_principal_id: document.actor,
            request_id,
            idempotency: cancellation_idempotency(&document.cancellation_input)?,
        })
        .await?)
}

fn start_idempotency(input: &WorkflowRunInput) -> Result<IdempotencyRequest, String> {
    IdempotencyRequest::new(
        format!(
            "test.workflow-run.process-death.starts/{}",
            input.organization_id
        ),
        input.workflow_run_id.to_string(),
        &input.canonical_bytes()?,
    )
}

fn cancellation_idempotency(input: &WorkflowRunInput) -> TestResult<IdempotencyRequest> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "organizationId": input.organization_id,
        "workflowRunId": input.workflow_run_id,
        "reason": CANCELLATION_REASON,
    }))?;
    Ok(IdempotencyRequest::new(
        format!(
            "test.workflow-run.process-death.cancellations/{}",
            input.organization_id
        ),
        input.workflow_run_id.to_string(),
        &canonical,
    )?)
}
