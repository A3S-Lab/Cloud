use super::{Fixture, TestResult};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    AgentConversationId, AgentExecutionCheckpointId, AgentExecutionId, OrganizationId,
};
use serde_json::Value;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROBE_TEST: &str = "agent_checkpoint_postgres_process_death_probe";
const PROBE_PARENT_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_PARENT";
const PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_POSTGRES_URL";
const PROBE_STATE_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_STATE_DIR";
const PROBE_OBJECTS_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_OBJECTS_DIR";
const PROBE_ORGANIZATION_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_ORGANIZATION_ID";
const PROBE_CONVERSATION_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_CONVERSATION_ID";
const PROBE_EXECUTION_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_EXECUTION_ID";
const PROBE_CHECKPOINT_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_CHECKPOINT_ID";
const PROBE_MODE_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_MODE";
const PROBE_MARKER_ENV: &str = "A3S_CLOUD_AGENT_CHECKPOINT_CRASH_MARKER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeMode {
    ObjectCommitted,
    ForkCommitted,
}

impl ProbeMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::ObjectCommitted => "object-committed",
            Self::ForkCommitted => "fork-committed",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "object-committed" => Ok(Self::ObjectCommitted),
            "fork-committed" => Ok(Self::ForkCommitted),
            _ => Err(format!(
                "unknown Agent checkpoint crash probe mode {value:?}"
            )),
        }
    }
}

pub(super) struct ProbeEnvironment {
    pub(super) postgres_url: String,
    pub(super) state_dir: PathBuf,
    pub(super) objects_dir: PathBuf,
    pub(super) organization_id: OrganizationId,
    pub(super) conversation_id: AgentConversationId,
    pub(super) execution_id: AgentExecutionId,
    pub(super) checkpoint_id: Option<AgentExecutionCheckpointId>,
    pub(super) mode: ProbeMode,
    pub(super) marker: PathBuf,
}

pub(super) fn probe_environment() -> TestResult<ProbeEnvironment> {
    if require_probe_environment(PROBE_PARENT_ENV)? != "1" {
        return Err("Agent checkpoint crash probe requires its private parent marker".into());
    }
    let checkpoint_id = require_probe_environment(PROBE_CHECKPOINT_ENV)?;
    Ok(ProbeEnvironment {
        postgres_url: require_probe_environment(PROBE_POSTGRES_ENV)?,
        state_dir: PathBuf::from(require_probe_environment(PROBE_STATE_ENV)?),
        objects_dir: PathBuf::from(require_probe_environment(PROBE_OBJECTS_ENV)?),
        organization_id: OrganizationId::from_uuid(Uuid::parse_str(&require_probe_environment(
            PROBE_ORGANIZATION_ENV,
        )?)?),
        conversation_id: AgentConversationId::from_uuid(Uuid::parse_str(
            &require_probe_environment(PROBE_CONVERSATION_ENV)?,
        )?),
        execution_id: AgentExecutionId::from_uuid(Uuid::parse_str(&require_probe_environment(
            PROBE_EXECUTION_ENV,
        )?)?),
        checkpoint_id: if checkpoint_id.is_empty() {
            None
        } else {
            Some(AgentExecutionCheckpointId::from_uuid(Uuid::parse_str(
                &checkpoint_id,
            )?))
        },
        mode: ProbeMode::parse(&require_probe_environment(PROBE_MODE_ENV)?)?,
        marker: PathBuf::from(require_probe_environment(PROBE_MARKER_ENV)?),
    })
}

pub(super) async fn crash_at(
    fixture: &Fixture,
    index: usize,
    mode: ProbeMode,
    checkpoint_id: Option<AgentExecutionCheckpointId>,
) -> TestResult<Value> {
    let marker = fixture
        .state_dir
        .join(format!("crash-{index:02}-{}.json", mode.as_str()));
    if marker.exists() {
        return Err(format!("crash marker already exists: {}", marker.display()).into());
    }
    let mut probe = CrashProbeProcess::start(fixture, mode, checkpoint_id, &marker)?;
    wait_for_marker(&mut probe, &marker, mode).await?;
    let document = serde_json::from_slice(&std::fs::read(&marker)?)?;
    let status = probe.kill_and_wait()?;
    require_sigkill(status)?;
    Ok(document)
}

pub(super) fn publish_marker(path: &Path, marker: &Value) -> TestResult {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Agent checkpoint crash marker path has no UTF-8 file name")?;
    let pending = path.with_file_name(format!(".{file_name}.{}.pending", std::process::id()));
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)?;
    serde_json::to_writer(&file, marker)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = std::fs::rename(&pending, path) {
        let _ = std::fs::remove_file(&pending);
        return Err(error.into());
    }
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
                "Agent checkpoint crash probe {} exited with {status} before publishing its marker",
                mode.as_str()
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Agent checkpoint crash probe {} did not reach its boundary in 60 seconds",
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
    fn start(
        fixture: &Fixture,
        mode: ProbeMode,
        checkpoint_id: Option<AgentExecutionCheckpointId>,
        marker: &Path,
    ) -> TestResult<Self> {
        let child = Command::new(std::env::current_exe()?)
            .arg(PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(PROBE_PARENT_ENV, "1")
            .env(PROBE_POSTGRES_ENV, &fixture.postgres_url)
            .env(PROBE_STATE_ENV, &fixture.state_dir)
            .env(PROBE_OBJECTS_ENV, &fixture.objects_dir)
            .env(PROBE_ORGANIZATION_ENV, fixture.organization_id.to_string())
            .env(PROBE_CONVERSATION_ENV, fixture.conversation_id.to_string())
            .env(PROBE_EXECUTION_ENV, fixture.execution_id.to_string())
            .env(
                PROBE_CHECKPOINT_ENV,
                checkpoint_id.map(|id| id.to_string()).unwrap_or_default(),
            )
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
            .ok_or_else(|| std::io::Error::other("Agent checkpoint crash probe disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("Agent checkpoint crash probe disappeared"))?;
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
        return Err(
            "Agent checkpoint crash probe exited successfully instead of being killed".into(),
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(format!(
                "Agent checkpoint crash probe exited with {status} instead of SIGKILL"
            )
            .into());
        }
    }
    Ok(())
}

fn require_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("Agent checkpoint crash probe omitted {name}")))
}
