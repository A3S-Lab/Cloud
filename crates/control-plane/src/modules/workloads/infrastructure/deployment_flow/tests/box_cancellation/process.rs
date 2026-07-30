use super::*;
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation, RuntimeRemoval,
};
use std::fs::OpenOptions;
use std::io::Write;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration as StdDuration;

const CRASH_PROBE_TEST: &str = "modules::workloads::infrastructure::deployment_flow::tests::box_cancellation::process::real_box_cleanup_crash_probe";
const CRASH_PROBE_PARENT_ENV: &str = "A3S_CLOUD_BOX_CLEANUP_CRASH_PROBE";
const CRASH_PROBE_RUNTIME_STATE_ENV: &str = "A3S_CLOUD_BOX_CLEANUP_RUNTIME_STATE";
const CRASH_PROBE_NODE_STATE_ENV: &str = "A3S_CLOUD_BOX_CLEANUP_NODE_STATE";
const CRASH_PROBE_COMMAND_ENV: &str = "A3S_CLOUD_BOX_CLEANUP_COMMAND";
const CRASH_PROBE_MARKER_ENV: &str = "A3S_CLOUD_BOX_CLEANUP_MARKER";

pub(super) async fn interrupt_after_provider_remove(
    home: &Path,
    runtime_state: &Path,
    node_state: &Path,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
) -> BoxTestResult<RuntimeRemoval> {
    let probe_state = tempfile::tempdir()?;
    let command_path = probe_state.path().join("command.json");
    let marker_path = probe_state.path().join("removed.json");
    let durable_command_path = command_path.clone();
    let command_body = serde_json::to_vec(command)?;
    tokio::task::spawn_blocking(move || write_durable_file(&durable_command_path, &command_body))
        .await??;

    let mut probe = CleanupCrashProbe::start(
        &std::env::current_exe()?,
        runtime_state,
        node_state,
        &command_path,
        &marker_path,
    )?;
    let removal = wait_for_remove_marker(&marker_path, &mut probe).await?;
    let status = probe.kill_and_wait()?;
    if status.success() {
        return Err(
            invalid("cleanup crash probe exited successfully instead of being killed").into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(invalid(format!(
                "cleanup crash probe exited with {status} instead of SIGKILL"
            ))
            .into());
        }
    }

    let interrupted_journal = FileCommandJournal::new(node_state, command.node_id)?;
    if interrupted_journal
        .pending_acknowledgements()
        .await?
        .iter()
        .any(|acknowledgement| acknowledgement.command_id == command.command_id)
    {
        return Err(invalid(
            "interrupted Agent journal projected a false Runtime removal acknowledgement",
        )
        .into());
    }
    let recovered_runtime = build_test_box_runtime(
        &BoxRuntimeConfig {
            home_dir: home.to_path_buf(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
        },
        runtime_state,
    )?;
    if !matches!(
        recovered_runtime.inspect(&removal.unit_id).await?,
        RuntimeInspection::NotFound { .. }
    ) {
        return Err(invalid(
            "Box provider cleanup was not durable before the Agent process interruption",
        )
        .into());
    }
    Ok(removal)
}

pub(super) async fn recover_interrupted_remove(
    home: &Path,
    runtime_state: &Path,
    node_state: &Path,
    command: &a3s_cloud_contracts::NodeCommandEnvelope,
    expected: &RuntimeRemoval,
) -> BoxTestResult<NodeCommandAck> {
    let runtime = build_test_box_runtime(
        &BoxRuntimeConfig {
            home_dir: home.to_path_buf(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
        },
        runtime_state,
    )?;
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(node_state, command.node_id)?,
        runtime,
    );
    let acknowledgement = executor.execute(command.clone()).await?;
    let recovered = removal_result(&acknowledgement)?;
    if recovered != expected {
        return Err(invalid(
            "Agent recovery did not adopt the exact durable Runtime removal receipt",
        )
        .into());
    }
    Ok(acknowledgement)
}

#[tokio::test]
#[ignore = "private subprocess used only by the real Box cleanup crash gate"]
async fn real_box_cleanup_crash_probe() -> BoxTestResult<()> {
    if required_environment(CRASH_PROBE_PARENT_ENV)? != "1" {
        return Err(invalid("cleanup crash probe requires its private parent marker").into());
    }
    let runtime_state = PathBuf::from(required_environment(CRASH_PROBE_RUNTIME_STATE_ENV)?);
    let node_state = PathBuf::from(required_environment(CRASH_PROBE_NODE_STATE_ENV)?);
    let command_path = PathBuf::from(required_environment(CRASH_PROBE_COMMAND_ENV)?);
    let marker_path = PathBuf::from(required_environment(CRASH_PROBE_MARKER_ENV)?);
    let command: a3s_cloud_contracts::NodeCommandEnvelope =
        serde_json::from_slice(&tokio::fs::read(command_path).await?)?;
    command.validate().map_err(invalid)?;
    if !matches!(command.payload, NodeCommandPayload::RuntimeRemove { .. }) {
        return Err(invalid("cleanup crash probe requires a Runtime remove command").into());
    }

    let home = dedicated_box_home()?;
    let runtime = build_test_box_runtime(
        &BoxRuntimeConfig {
            home_dir: home.clone(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
        },
        &runtime_state,
    )?;
    let paused_runtime: Arc<dyn RuntimeClient> = Arc::new(PauseAfterRemove {
        inner: runtime,
        marker_path,
    });
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(node_state, command.node_id)?,
        paused_runtime,
    );
    let result = executor.execute(command).await;
    Err(invalid(format!(
        "cleanup crash probe returned before process death: {result:?}"
    ))
    .into())
}

struct PauseAfterRemove {
    inner: Arc<dyn RuntimeClient>,
    marker_path: PathBuf,
}

#[async_trait]
impl RuntimeClient for PauseAfterRemove {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        self.inner.capabilities().await
    }

    async fn apply(&self, request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        self.inner.apply(request).await
    }

    async fn inspect(&self, unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        self.inner.inspect(unit_id).await
    }

    async fn stop(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        self.inner.stop(request).await
    }

    async fn remove(&self, request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        let removal = self.inner.remove(request).await?;
        let body = serde_json::to_vec(&removal).map_err(|error| {
            a3s_runtime::RuntimeError::Protocol(format!(
                "could not encode cleanup crash marker: {error}"
            ))
        })?;
        let marker_path = self.marker_path.clone();
        tokio::task::spawn_blocking(move || write_durable_file(&marker_path, &body))
            .await
            .map_err(|error| {
                a3s_runtime::RuntimeError::ProviderUnavailable(format!(
                    "cleanup crash marker task failed: {error}"
                ))
            })?
            .map_err(|error| {
                a3s_runtime::RuntimeError::ProviderUnavailable(format!(
                    "could not persist cleanup crash marker: {error}"
                ))
            })?;
        std::future::pending::<RuntimeResult<RuntimeRemoval>>().await
    }

    async fn logs(&self, query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        self.inner.logs(query).await
    }

    async fn exec(&self, request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        self.inner.exec(request).await
    }
}

struct CleanupCrashProbe {
    child: Option<Child>,
}

impl CleanupCrashProbe {
    fn start(
        test_binary: &Path,
        runtime_state: &Path,
        node_state: &Path,
        command_path: &Path,
        marker_path: &Path,
    ) -> BoxTestResult<Self> {
        let child = Command::new(test_binary)
            .arg(CRASH_PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_PROBE_PARENT_ENV, "1")
            .env(CRASH_PROBE_RUNTIME_STATE_ENV, runtime_state)
            .env(CRASH_PROBE_NODE_STATE_ENV, node_state)
            .env(CRASH_PROBE_COMMAND_ENV, command_path)
            .env(CRASH_PROBE_MARKER_ENV, marker_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| io::Error::other("cleanup crash probe process disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("cleanup crash probe process disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CleanupCrashProbe {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn wait_for_remove_marker(
    marker_path: &Path,
    probe: &mut CleanupCrashProbe,
) -> BoxTestResult<RuntimeRemoval> {
    for _ in 0..2_400 {
        match tokio::fs::read(marker_path).await {
            Ok(body) => return Ok(serde_json::from_slice(&body)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(status) = probe.try_wait()? {
            return Err(invalid(format!(
                "cleanup crash probe exited before provider removal with {status}"
            ))
            .into());
        }
        tokio::time::sleep(StdDuration::from_millis(50)).await;
    }
    Err(invalid("cleanup crash probe did not remove the Box resource within 120 seconds").into())
}

fn removal_result(acknowledgement: &NodeCommandAck) -> BoxTestResult<&RuntimeRemoval> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            a3s_cloud_contracts::NodeCommandResult::RuntimeRemoved { removal } => Ok(removal),
            result => Err(invalid(format!(
                "recovered cleanup returned an unexpected result: {result:?}"
            ))
            .into()),
        },
        outcome => Err(invalid(format!("recovered cleanup did not succeed: {outcome:?}")).into()),
    }
}

fn write_durable_file(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("durable cleanup probe file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".cleanup-probe-{}.tmp", Uuid::now_v7()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(body)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)?;
    std::fs::File::open(parent)?.sync_all()
}

fn required_environment(name: &str) -> BoxTestResult<String> {
    std::env::var(name).map_err(|_| invalid(format!("cleanup crash probe omitted {name}")).into())
}
