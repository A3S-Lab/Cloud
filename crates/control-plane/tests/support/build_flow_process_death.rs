#[path = "build_flow_process_death_evidence.rs"]
mod evidence;
#[path = "build_flow_process_death_fixture.rs"]
mod fixture;
#[path = "build_flow_process_death_runtime.rs"]
mod runtime;

use a3s_cloud_contracts::{
    NodeBoxBuildCancelResult, NodeBoxBuildCancellation, NodeBoxBuildInspection,
    NodeBoxBuildOperationCancellation, NodeBoxBuildOperationRemoval, NodeBoxBuildPhase,
    NodeBoxBuildRemoveResult, NodeBoxBuildStartResult, NodeCommandResult,
};
use a3s_cloud_control_plane::infrastructure::connect_and_migrate;
use a3s_cloud_control_plane::modules::artifacts::{BuildRunStatus, IBuildRunRepository};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    BuildRunId, NodeCommandId, OrganizationId,
};
use a3s_flow::{FlowEngine, FlowEvent, FlowEventStore, WorkflowRunSnapshot, WorkflowRunStatus};
use a3s_orm::PostgresExecutor;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use fixture::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const PROBE_TEST: &str = "build_flow_postgres_process_death_probe";
const PROBE_PARENT_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_POSTGRES_URL";
const PROBE_STATE_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_STATE_DIR";
const PROBE_ORGANIZATION_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_ORGANIZATION_ID";
const PROBE_BUILD_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_BUILD_RUN_ID";
const PROBE_TARGET_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_TARGET_STEP";
const PROBE_MODE_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_MODE";
const PROBE_MARKER_ENV: &str = "A3S_CLOUD_BUILD_FLOW_CRASH_MARKER";

const INTERRUPTED_STEPS: [&str; 9] = [
    "dispatch",
    "observe-1-2",
    "observe-1-3",
    "cleanup-cancel-dispatch-1",
    "cleanup-cancel-observe-1-2",
    "cleanup-inspect-dispatch-2",
    "cleanup-inspect-observe-2-2",
    "cleanup-remove-dispatch-3",
    "cleanup-remove-observe-3-2",
];

#[derive(Clone, Copy)]
enum ProbeMode {
    Start,
    ResumeWait,
    StartThenResumeWait,
}

impl ProbeMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::ResumeWait => "resume-wait",
            Self::StartThenResumeWait => "start-then-resume-wait",
        }
    }
}

pub async fn exercise_process_death_matrix(postgres_url: String) -> TestResult {
    let executor = connect_and_migrate(&postgres_url, 8).await?;
    let state = tempfile::tempdir()?;
    let fixture = setup_fixture(executor, postgres_url, state.path()).await?;
    runtime::postgres_flow_store(&fixture.postgres_url).await?;

    crash_at(&fixture, 1, "dispatch", ProbeMode::Start).await?;
    let start_id = NodeCommandId::from_uuid(Uuid::new_v5(
        &fixture.build.id.as_uuid(),
        b"box-build-start",
    ));
    let start_before = find_command(&fixture, start_id, "Box start before restart").await?;
    let dispatched = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    if dispatched.command_id != Some(start_id) {
        return Err("Box start command identity was not committed before process death".into());
    }
    recover_start(&fixture).await?;
    let start_after = find_command(&fixture, start_id, "Box start after restart").await?;
    require_same_command(&start_before, &start_after, "Box start")?;

    let start = lease_one(&fixture, 0).await?;
    let build_request = request(&start)?.clone();
    acknowledge(
        &fixture,
        &start,
        NodeCommandResult::BoxBuildStarted {
            started: NodeBoxBuildStartResult {
                binding_digest: build_request.binding_digest()?,
                phase: NodeBoxBuildPhase::Running,
            },
        },
    )
    .await?;

    crash_at(&fixture, 2, "observe-1-2", ProbeMode::ResumeWait).await?;
    let inspect_id = NodeCommandId::from_uuid(Uuid::new_v5(
        &fixture.build.id.as_uuid(),
        b"box-build-inspect:1",
    ));
    let inspect_before =
        find_command(&fixture, inspect_id, "Box inspection before restart").await?;
    recover_start(&fixture).await?;
    let inspect_after = find_command(&fixture, inspect_id, "Box inspection after restart").await?;
    require_same_command(&inspect_before, &inspect_after, "Box inspection")?;

    let inspect = lease_one(&fixture, start.sequence).await?;
    let output = box_output_for(&build_request, runtime::output_artifact()?)?;
    acknowledge(
        &fixture,
        &inspect,
        NodeCommandResult::BoxBuildInspected {
            inspection: Box::new(NodeBoxBuildInspection::Succeeded {
                binding_digest: build_request.binding_digest()?,
                output: Box::new(output.clone()),
            }),
        },
    )
    .await?;

    crash_at(&fixture, 3, "observe-1-3", ProbeMode::ResumeWait).await?;
    let receipt_before = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?
        .box_build_output
        .ok_or("Box output receipt was not committed before process death")?;
    if receipt_before != output {
        return Err("persisted Box output receipt changed before Flow completion".into());
    }

    crash_at(&fixture, 4, "cleanup-cancel-dispatch-1", ProbeMode::Start).await?;
    let receipt_after = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?
        .box_build_output
        .ok_or("Box output receipt disappeared during restart")?;
    if receipt_after != receipt_before {
        return Err("Box output receipt changed while replaying its lost Flow completion".into());
    }
    let cancel_id = cleanup_command_id(&fixture, "Box cancellation").await?;
    let cancel_before =
        find_command(&fixture, cancel_id, "Box cancellation before restart").await?;
    recover_start(&fixture).await?;
    let cancel_after = find_command(&fixture, cancel_id, "Box cancellation after restart").await?;
    require_same_command(&cancel_before, &cancel_after, "Box cancellation")?;

    let cancel = lease_one(&fixture, inspect.sequence).await?;
    acknowledge(
        &fixture,
        &cancel,
        NodeCommandResult::BoxBuildCancelled {
            cancelled: NodeBoxBuildCancelResult {
                binding_digest: build_request.binding_digest()?,
                operations: build_request
                    .plans
                    .iter()
                    .map(|plan| NodeBoxBuildOperationCancellation {
                        operation_id: plan.operation_id.clone(),
                        outcome: NodeBoxBuildCancellation::Requested,
                    })
                    .collect(),
            },
        },
    )
    .await?;

    crash_at(
        &fixture,
        5,
        "cleanup-cancel-observe-1-2",
        ProbeMode::ResumeWait,
    )
    .await?;
    crash_at(
        &fixture,
        6,
        "cleanup-inspect-dispatch-2",
        ProbeMode::StartThenResumeWait,
    )
    .await?;
    let cleanup_inspect_id = cleanup_command_id(&fixture, "Box cleanup inspection").await?;
    let cleanup_inspect_before = find_command(
        &fixture,
        cleanup_inspect_id,
        "Box cleanup inspection before restart",
    )
    .await?;
    recover_start(&fixture).await?;
    let cleanup_inspect_after = find_command(
        &fixture,
        cleanup_inspect_id,
        "Box cleanup inspection after restart",
    )
    .await?;
    require_same_command(
        &cleanup_inspect_before,
        &cleanup_inspect_after,
        "Box cleanup inspection",
    )?;

    let cleanup_inspect = lease_one(&fixture, cancel.sequence).await?;
    acknowledge(
        &fixture,
        &cleanup_inspect,
        NodeCommandResult::BoxBuildInspected {
            inspection: Box::new(NodeBoxBuildInspection::Succeeded {
                binding_digest: build_request.binding_digest()?,
                output: Box::new(output.clone()),
            }),
        },
    )
    .await?;

    crash_at(
        &fixture,
        7,
        "cleanup-inspect-observe-2-2",
        ProbeMode::ResumeWait,
    )
    .await?;
    crash_at(
        &fixture,
        8,
        "cleanup-remove-dispatch-3",
        ProbeMode::StartThenResumeWait,
    )
    .await?;
    let remove_id = cleanup_command_id(&fixture, "Box removal").await?;
    let remove_before = find_command(&fixture, remove_id, "Box removal before restart").await?;
    recover_start(&fixture).await?;
    let remove_after = find_command(&fixture, remove_id, "Box removal after restart").await?;
    require_same_command(&remove_before, &remove_after, "Box removal")?;

    let remove = lease_one(&fixture, cleanup_inspect.sequence).await?;
    acknowledge(
        &fixture,
        &remove,
        NodeCommandResult::BoxBuildRemoved {
            removed: NodeBoxBuildRemoveResult {
                binding_digest: build_request.binding_digest()?,
                operations: build_request
                    .plans
                    .iter()
                    .map(|plan| NodeBoxBuildOperationRemoval {
                        operation_id: plan.operation_id.clone(),
                        removed: true,
                    })
                    .collect(),
                assembly_removed: build_request.assembly_reference.is_some(),
            },
        },
    )
    .await?;

    crash_at(
        &fixture,
        9,
        "cleanup-remove-observe-3-2",
        ProbeMode::ResumeWait,
    )
    .await?;
    let snapshot = recover_start(&fixture).await?;
    if snapshot.status != WorkflowRunStatus::Completed {
        return Err(format!(
            "restarted Build Flow finished in {:?} instead of completed",
            snapshot.status
        )
        .into());
    }
    let completed = fixture
        .builds
        .find(fixture.organization_id, fixture.build.id)
        .await?;
    if completed.status != BuildRunStatus::Succeeded
        || completed.published_artifact.is_none()
        || completed.evidence.is_none()
    {
        return Err("restarted BuildRun omitted its successful publication or evidence".into());
    }
    if !(start.sequence < inspect.sequence
        && inspect.sequence < cancel.sequence
        && cancel.sequence < cleanup_inspect.sequence
        && cleanup_inspect.sequence < remove.sequence)
    {
        return Err("replayed Fleet command sequence is not strictly monotonic".into());
    }
    verify_final_history(&fixture).await?;
    verify_action_counts(&fixture.state_dir)?;
    verify_command_count(&fixture, 5).await?;
    println!(
        "A3S_CLOUD_G0_FLEET_FLOW_PROCESS_DEATH_CERTIFIED boundaries={} sigkills={} commands={} status=succeeded store=postgresql start={} cancel={} inspect={} remove={}",
        INTERRUPTED_STEPS.len(),
        INTERRUPTED_STEPS.len(),
        5,
        start.command_id,
        cancel.command_id,
        cleanup_inspect.command_id,
        remove.command_id,
    );
    Ok(())
}

pub async fn run_probe() -> TestResult {
    if require_probe_environment(PROBE_PARENT_ENV)? != "1" {
        return Err("Build Flow crash probe requires its private parent marker".into());
    }
    let postgres_url = require_probe_environment(PROBE_POSTGRES_ENV)?;
    let state_dir = PathBuf::from(require_probe_environment(PROBE_STATE_ENV)?);
    let organization_id = OrganizationId::from_uuid(Uuid::parse_str(&require_probe_environment(
        PROBE_ORGANIZATION_ENV,
    )?)?);
    let build_run_id = BuildRunId::from_uuid(Uuid::parse_str(&require_probe_environment(
        PROBE_BUILD_ENV,
    )?)?);
    let target = require_probe_environment(PROBE_TARGET_ENV)?;
    let marker = PathBuf::from(require_probe_environment(PROBE_MARKER_ENV)?);
    let mode = require_probe_environment(PROBE_MODE_ENV)?;
    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 4)?;
    let flow_store = runtime::postgres_flow_store(&postgres_url).await?;
    let crash_store =
        runtime::CrashBeforeStepCompletionStore::new(flow_store, target.clone(), marker);
    let engine = FlowEngine::new(
        Arc::new(crash_store),
        Arc::new(runtime::build_runtime(executor, &state_dir).await?),
    );
    let run_id = build_run_id.to_string();
    match mode.as_str() {
        "start" | "start-then-resume-wait" => {
            engine
                .start_with_id(
                    run_id.clone(),
                    runtime::workflow_spec(),
                    runtime::flow_input(organization_id, build_run_id),
                )
                .await?;
            if mode == "start-then-resume-wait" {
                engine
                    .resume_due_waits(Utc::now() + Duration::days(1))
                    .await?;
            }
        }
        "resume-wait" => {
            engine
                .resume_due_waits(Utc::now() + Duration::days(1))
                .await?;
        }
        _ => return Err(format!("unknown Build Flow crash probe mode {mode}").into()),
    }
    Err(format!("Build Flow crash probe returned without pausing at {target} completion").into())
}

async fn recover_start(fixture: &Fixture) -> TestResult<WorkflowRunSnapshot> {
    let store = runtime::postgres_flow_store(&fixture.postgres_url).await?;
    let engine = FlowEngine::new(
        Arc::new(store),
        Arc::new(runtime::build_runtime(fixture.executor.clone(), &fixture.state_dir).await?),
    );
    let run_id = fixture.build.operation_id.to_string();
    engine
        .start_with_id(
            run_id.clone(),
            runtime::workflow_spec(),
            runtime::flow_input(fixture.organization_id, fixture.build.id),
        )
        .await?;
    Ok(engine.snapshot(&run_id).await?)
}

async fn crash_at(fixture: &Fixture, index: usize, target: &str, mode: ProbeMode) -> TestResult {
    let marker = fixture
        .state_dir
        .join(format!("crash-{index:02}-{target}.json"));
    if marker.exists() {
        return Err(format!("crash marker already exists: {}", marker.display()).into());
    }
    assert_step_completion_absent(fixture, target).await?;
    let mut probe = CrashProbeProcess::start(fixture, target, mode, &marker)?;
    wait_for_marker(&mut probe, &marker, target).await?;
    verify_crash_marker(fixture, &marker, target)?;
    assert_step_completion_absent(fixture, target).await?;
    let status = probe.kill_and_wait()?;
    require_sigkill(status)?;
    assert_step_completion_absent(fixture, target).await?;
    Ok(())
}

fn verify_crash_marker(fixture: &Fixture, marker: &Path, target: &str) -> TestResult {
    let document = serde_json::from_slice::<serde_json::Value>(&std::fs::read(marker)?)?;
    let expected_run_id = fixture.build.operation_id.to_string();
    if document["runId"].as_str() != Some(expected_run_id.as_str())
        || document["stepId"].as_str() != Some(target)
        || document["expectedSequence"]
            .as_u64()
            .is_none_or(|sequence| sequence == 0)
    {
        return Err(format!(
            "Build Flow crash marker for {target} did not bind its run and event sequence"
        )
        .into());
    }
    Ok(())
}

async fn wait_for_marker(probe: &mut CrashProbeProcess, marker: &Path, target: &str) -> TestResult {
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if marker.is_file() {
            return Ok(());
        }
        if let Some(status) = probe.try_wait()? {
            return Err(format!(
                "Build Flow crash probe exited with {status} before reaching {target}"
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(
                format!("Build Flow crash probe did not reach {target} in 60 seconds").into(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

async fn assert_step_completion_absent(fixture: &Fixture, target: &str) -> TestResult {
    let store = runtime::postgres_flow_store(&fixture.postgres_url).await?;
    let history = store.list(&fixture.build.operation_id.to_string()).await;
    let history = match history {
        Ok(history) => history,
        Err(a3s_flow::FlowError::RunNotFound(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if history.iter().any(|envelope| {
        matches!(
            &envelope.event,
            FlowEvent::StepCompleted { step_id, .. } if step_id == target
        )
    }) {
        return Err(format!(
            "Flow completion for {target} was persisted before the crash probe died"
        )
        .into());
    }
    Ok(())
}

async fn verify_final_history(fixture: &Fixture) -> TestResult {
    let store = runtime::postgres_flow_store(&fixture.postgres_url).await?;
    let history = store.list(&fixture.build.operation_id.to_string()).await?;
    for target in INTERRUPTED_STEPS {
        let completions = history
            .iter()
            .filter(|envelope| {
                matches!(
                    &envelope.event,
                    FlowEvent::StepCompleted { step_id, .. } if step_id == target
                )
            })
            .count();
        if completions != 1 {
            return Err(format!(
                "recovered Flow persisted {completions} completions for {target}, expected one"
            )
            .into());
        }
    }
    Ok(())
}

fn verify_action_counts(state_dir: &Path) -> TestResult {
    let counts = runtime::action_counts(state_dir)?;
    let expected = BTreeMap::from([
        ("evidence".to_owned(), 1_usize),
        ("input.prepare".to_owned(), 1),
        ("input.remove".to_owned(), 1),
        ("output.validate".to_owned(), 1),
        ("publication.publish".to_owned(), 1),
        ("publication.target".to_owned(), 1),
    ]);
    if counts != expected {
        return Err(format!(
            "Build Flow side effects were not logically exact once: expected {expected:?}, got {counts:?}"
        )
        .into());
    }
    Ok(())
}

struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    fn start(fixture: &Fixture, target: &str, mode: ProbeMode, marker: &Path) -> TestResult<Self> {
        let child = Command::new(std::env::current_exe()?)
            .arg(PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(PROBE_PARENT_ENV, "1")
            .env(PROBE_POSTGRES_ENV, &fixture.postgres_url)
            .env(PROBE_STATE_ENV, &fixture.state_dir)
            .env(PROBE_ORGANIZATION_ENV, fixture.organization_id.to_string())
            .env(PROBE_BUILD_ENV, fixture.build.id.to_string())
            .env(PROBE_TARGET_ENV, target)
            .env(PROBE_MODE_ENV, mode.as_str())
            .env(PROBE_MARKER_ENV, marker)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Build Flow crash probe disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("Build Flow crash probe disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CrashProbeProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn require_sigkill(status: ExitStatus) -> TestResult {
    if status.success() {
        return Err("Build Flow crash probe exited successfully instead of being killed".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(
                format!("Build Flow crash probe exited with {status} instead of SIGKILL").into(),
            );
        }
    }
    Ok(())
}

fn require_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("Build Flow crash probe omitted {name}")))
}
