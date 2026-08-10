use super::fixture::Fixture;
use super::TestResult;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const PROBE_TEST: &str = "workflow_run_postgres_process_death_probe";
const PROBE_PARENT_ENV: &str = "A3S_CLOUD_WORKFLOW_RUN_CRASH_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_WORKFLOW_RUN_CRASH_POSTGRES_URL";
const PROBE_STATE_ENV: &str = "A3S_CLOUD_WORKFLOW_RUN_CRASH_STATE_DIR";
const PROBE_MODE_ENV: &str = "A3S_CLOUD_WORKFLOW_RUN_CRASH_MODE";
const PROBE_MARKER_ENV: &str = "A3S_CLOUD_WORKFLOW_RUN_CRASH_MARKER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeMode {
    CreateCommit,
    FlowStarted,
    TerminalObserved,
    CancellationCommit,
}

impl ProbeMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::CreateCommit => "create-commit",
            Self::FlowStarted => "flow-started",
            Self::TerminalObserved => "terminal-observed",
            Self::CancellationCommit => "cancellation-commit",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "create-commit" => Ok(Self::CreateCommit),
            "flow-started" => Ok(Self::FlowStarted),
            "terminal-observed" => Ok(Self::TerminalObserved),
            "cancellation-commit" => Ok(Self::CancellationCommit),
            _ => Err(format!("unknown WorkflowRun crash probe mode {value:?}")),
        }
    }
}

pub(super) struct ProbeEnvironment {
    pub(super) postgres_url: String,
    pub(super) state_dir: PathBuf,
    pub(super) mode: ProbeMode,
    pub(super) marker: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CrashMarker<'a> {
    pub(super) mode: &'a str,
    pub(super) workflow_run_id: String,
    pub(super) operation_id: String,
    pub(super) flow_run_id: String,
    pub(super) status: &'a str,
    pub(super) aggregate_version: u64,
    pub(super) last_flow_sequence: u64,
}

pub(super) fn probe_environment() -> TestResult<ProbeEnvironment> {
    if require_probe_environment(PROBE_PARENT_ENV)? != "1" {
        return Err("WorkflowRun crash probe requires its private parent marker".into());
    }
    Ok(ProbeEnvironment {
        postgres_url: require_probe_environment(PROBE_POSTGRES_ENV)?,
        state_dir: PathBuf::from(require_probe_environment(PROBE_STATE_ENV)?),
        mode: ProbeMode::parse(&require_probe_environment(PROBE_MODE_ENV)?)?,
        marker: PathBuf::from(require_probe_environment(PROBE_MARKER_ENV)?),
    })
}

pub(super) async fn crash_at(
    fixture: &Fixture,
    index: usize,
    mode: ProbeMode,
) -> TestResult<serde_json::Value> {
    let marker = fixture
        .state_dir
        .join(format!("crash-{index:02}-{}.json", mode.as_str()));
    if marker.exists() {
        return Err(format!("crash marker already exists: {}", marker.display()).into());
    }
    let mut probe = CrashProbeProcess::start(fixture, mode, &marker)?;
    wait_for_marker(&mut probe, &marker, mode).await?;
    let document = serde_json::from_slice(&std::fs::read(&marker)?)?;
    let status = probe.kill_and_wait()?;
    require_sigkill(status)?;
    Ok(document)
}

pub(super) fn publish_marker(path: &Path, marker: CrashMarker<'_>) -> TestResult {
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    serde_json::to_writer(&file, &marker)?;
    file.sync_all()?;
    Ok(())
}

async fn wait_for_marker(
    probe: &mut CrashProbeProcess,
    marker: &Path,
    mode: ProbeMode,
) -> TestResult {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if marker.is_file() {
            return Ok(());
        }
        if let Some(status) = probe.try_wait()? {
            return Err(format!(
                "WorkflowRun crash probe {} exited with {status} before publishing its marker",
                mode.as_str()
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "WorkflowRun crash probe {} did not reach its boundary in 60 seconds",
                mode.as_str()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    fn start(fixture: &Fixture, mode: ProbeMode, marker: &Path) -> TestResult<Self> {
        let child = Command::new(std::env::current_exe()?)
            .arg(PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(PROBE_PARENT_ENV, "1")
            .env(PROBE_POSTGRES_ENV, &fixture.postgres_url)
            .env(PROBE_STATE_ENV, &fixture.state_dir)
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
            .ok_or_else(|| std::io::Error::other("WorkflowRun crash probe disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("WorkflowRun crash probe disappeared"))?;
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
        return Err("WorkflowRun crash probe exited successfully instead of being killed".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(
                format!("WorkflowRun crash probe exited with {status} instead of SIGKILL").into(),
            );
        }
    }
    Ok(())
}

fn require_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("WorkflowRun crash probe omitted {name}")))
}
