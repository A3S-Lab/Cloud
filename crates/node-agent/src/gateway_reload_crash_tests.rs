use super::*;
use crate::{CommandExecutor, FileCommandJournal};
use a3s_cloud_contracts::{
    GatewayAckState, NodeCommandAckReceipt, NodeCommandEnvelope, NodeCommandMetadata,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult,
};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
    RuntimeRemoval,
};
use a3s_runtime::{RuntimeClient, RuntimeError, RuntimeResult};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const GATEWAY_TOKEN: &str = "a3s-cloud-gateway-integration-token";
const CRASH_PROBE_TEST: &str =
    "gateway::reload_crash_tests::gateway_apply_before_acknowledgement_crash_probe";
const CRASH_PROBE_PARENT_ENV: &str = "A3S_CLOUD_GATEWAY_CRASH_PROBE_PARENT";
const CRASH_PROBE_BASE_URL_ENV: &str = "A3S_CLOUD_GATEWAY_CRASH_PROBE_BASE_URL";
const CRASH_PROBE_STATE_DIR_ENV: &str = "A3S_CLOUD_GATEWAY_CRASH_PROBE_STATE_DIR";
const CRASH_PROBE_COMMAND_ENV: &str = "A3S_CLOUD_GATEWAY_CRASH_PROBE_COMMAND";
const CRASH_PROBE_LOG_ENV: &str = "A3S_CLOUD_GATEWAY_CRASH_PROBE_LOG";

struct UnusedRuntime;

fn unused_runtime() -> RuntimeError {
    RuntimeError::Protocol("Gateway crash probe does not use Runtime".into())
}

#[async_trait]
impl RuntimeClient for UnusedRuntime {
    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Err(unused_runtime())
    }

    async fn apply(&self, _request: &RuntimeApplyRequest) -> RuntimeResult<RuntimeObservation> {
        Err(unused_runtime())
    }

    async fn inspect(&self, _unit_id: &str) -> RuntimeResult<RuntimeInspection> {
        Err(unused_runtime())
    }

    async fn stop(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeInspection> {
        Err(unused_runtime())
    }

    async fn remove(&self, _request: &RuntimeActionRequest) -> RuntimeResult<RuntimeRemoval> {
        Err(unused_runtime())
    }

    async fn logs(&self, _query: &RuntimeLogQuery) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        Err(unused_runtime())
    }

    async fn exec(&self, _request: &RuntimeExecRequest) -> RuntimeResult<RuntimeExecResult> {
        Err(unused_runtime())
    }
}

struct RecordedGatewayControl {
    inner: Arc<GatewayManagementClient>,
    apply_log: PathBuf,
    pause_after_apply: bool,
}

impl RecordedGatewayControl {
    async fn record_apply(&self, replayed: bool) -> Result<(), GatewayControlError> {
        let mut options = tokio::fs::OpenOptions::new();
        options.create(true).append(true);
        let mut file = options.open(&self.apply_log).await.map_err(|error| {
            GatewayControlError::Unavailable(format!(
                "could not open the Gateway apply crash marker: {error}"
            ))
        })?;
        let marker: &[u8] = if replayed { b"replay\n" } else { b"apply\n" };
        file.write_all(marker).await.map_err(|error| {
            GatewayControlError::Unavailable(format!(
                "could not write the Gateway apply crash marker: {error}"
            ))
        })?;
        file.sync_all().await.map_err(|error| {
            GatewayControlError::Unavailable(format!(
                "could not persist the Gateway apply crash marker: {error}"
            ))
        })
    }
}

#[async_trait]
impl GatewayControl for RecordedGatewayControl {
    async fn apply(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
        let status = self.inner.apply(snapshot).await?;
        self.record_apply(status.replayed).await?;
        if self.pause_after_apply {
            return std::future::pending().await;
        }
        Ok(status)
    }

    async fn readiness(
        &self,
        snapshot: &GatewaySnapshot,
    ) -> Result<ManagedSnapshotStatus, GatewayControlError> {
        self.inner.readiness(snapshot).await
    }
}

struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    fn start(
        test_binary: &Path,
        base_url: &str,
        state_dir: &Path,
        command_path: &Path,
        reload_log: &Path,
    ) -> std::io::Result<Self> {
        let child = Command::new(test_binary)
            .arg("--exact")
            .arg(CRASH_PROBE_TEST)
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_PROBE_PARENT_ENV, "1")
            .env(CRASH_PROBE_BASE_URL_ENV, base_url)
            .env(CRASH_PROBE_STATE_DIR_ENV, state_dir)
            .env(CRASH_PROBE_COMMAND_ENV, command_path)
            .env(CRASH_PROBE_LOG_ENV, reload_log)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Gateway crash probe process disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("Gateway crash probe process disappeared"))?;
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

struct GatewayProcess {
    child: Child,
}

impl GatewayProcess {
    fn start(binary: &str, config_path: &Path) -> std::io::Result<Self> {
        let child = Command::new(binary)
            .arg("--config")
            .arg(config_path)
            .env("A3S_GATEWAY_ADMIN_TOKEN", GATEWAY_TOKEN)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child })
    }
}

impl Drop for GatewayProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test]
#[ignore = "requires a dedicated remote Gateway runner"]
async fn installed_a3s_gateway_recovers_native_apply_after_agent_process_death() -> TestResult {
    let binary = required_gateway_binary()?;
    let directory = tempfile::tempdir()?;
    let (traffic_port, management_port) = unused_ports();
    let node_id = uuid::Uuid::now_v7();
    let managed_state_file = directory.path().join("managed-snapshot.json");
    let config_path = directory.path().join("gateway.acl");
    std::fs::write(
        &config_path,
        management_gateway_acl(management_port, node_id, &managed_state_file),
    )?;
    let mut gateway = GatewayProcess::start(&binary, &config_path)?;

    let base_url = format!("http://127.0.0.1:{management_port}/api/gateway");
    wait_for_gateway(&base_url, &mut gateway.child).await?;
    if tokio::net::TcpStream::connect(("127.0.0.1", traffic_port))
        .await
        .is_ok()
    {
        return Err("Gateway traffic port was available before the native apply".into());
    }

    let state_dir = directory.path().join("node-state");
    tokio::fs::create_dir(&state_dir).await?;
    let apply_log = state_dir.join("gateway-applies.log");
    let command_path = directory.path().join("gateway-command.json");
    let issued_at = Utc::now();
    let not_after = issued_at + ChronoDuration::minutes(10);
    let snapshot = GatewaySnapshot::new(
        node_id,
        1,
        None,
        issued_at,
        not_after,
        gateway_acl(
            traffic_port,
            management_port,
            node_id,
            &managed_state_file,
            1,
        ),
    )?;
    let command = NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: uuid::Uuid::now_v7(),
            lease_id: uuid::Uuid::now_v7(),
            node_id,
            sequence: 1,
            aggregate_id: node_id,
            issued_at,
            not_after,
            correlation_id: uuid::Uuid::now_v7(),
        },
        NodeCommandPayload::GatewaySnapshotInstall {
            snapshot: Box::new(snapshot.clone()),
        },
    )?;
    tokio::fs::write(&command_path, serde_json::to_vec(&command)?).await?;

    let mut crash_probe = CrashProbeProcess::start(
        &std::env::current_exe()?,
        &base_url,
        &state_dir,
        &command_path,
        &apply_log,
    )?;
    wait_for_apply_crash_marker(&apply_log, &mut crash_probe).await?;
    wait_for_tcp_listener(traffic_port, &mut gateway.child).await?;
    if !tokio::fs::try_exists(&managed_state_file).await? {
        return Err("Gateway native apply omitted its durable journal".into());
    }
    let crash_status = crash_probe.kill_and_wait()?;
    if crash_status.success() {
        return Err("Gateway crash probe exited successfully instead of being killed".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if crash_status.signal() != Some(9) {
            return Err(format!(
                "Gateway crash probe exited with {crash_status} instead of SIGKILL"
            )
            .into());
        }
    }
    if tokio::fs::read_to_string(&apply_log).await? != "apply\n" {
        return Err("Gateway crash probe did not reach one native apply".into());
    }

    let interrupted_journal = FileCommandJournal::new(state_dir.clone(), node_id)?;
    if interrupted_journal.after_sequence().await? != 0
        || !interrupted_journal
            .pending_acknowledgements()
            .await?
            .is_empty()
    {
        return Err("interrupted Gateway command journal projected a false acknowledgement".into());
    }

    let recovery_control: Arc<dyn GatewayControl> = Arc::new(RecordedGatewayControl {
        inner: gateway_control(&base_url)?,
        apply_log: apply_log.clone(),
        pause_after_apply: false,
    });
    let recovery_installer = Arc::new(DurableGatewaySnapshotInstaller::new(
        node_id,
        recovery_control,
    ));
    let recovery_executor = CommandExecutor::new(
        FileCommandJournal::new(state_dir.clone(), node_id)?,
        Arc::new(UnusedRuntime),
        recovery_installer.clone(),
    );
    let mut redelivered = command.clone();
    redelivered.lease_id = uuid::Uuid::now_v7();
    let recovered = recovery_executor.execute(redelivered.clone()).await?;
    recovered.validate_against(&redelivered)?;
    let gateway_acknowledgement = match &recovered.outcome {
        NodeCommandOutcome::Succeeded { result } => match result.as_ref() {
            NodeCommandResult::GatewaySnapshotInstalled { acknowledgement } => acknowledgement,
            _ => return Err("recovered Gateway command returned a Runtime result".into()),
        },
        _ => return Err("recovered Gateway command did not succeed".into()),
    };
    gateway_acknowledgement.validate_for(command.command_id, node_id, &snapshot)?;
    if gateway_acknowledgement.state != GatewayAckState::Applied || !gateway_acknowledgement.ready {
        return Err("recovered Gateway command did not produce an applied acknowledgement".into());
    }
    if tokio::fs::read_to_string(&apply_log).await? != "apply\nreplay\n" {
        return Err("Gateway recovery did not confirm one exact native replay".into());
    }
    drop(recovery_executor);

    let replay_control: Arc<dyn GatewayControl> = Arc::new(RecordedGatewayControl {
        inner: gateway_control(&base_url)?,
        apply_log: apply_log.clone(),
        pause_after_apply: false,
    });
    let replay_executor = CommandExecutor::new(
        FileCommandJournal::new(state_dir, node_id)?,
        Arc::new(UnusedRuntime),
        Arc::new(DurableGatewaySnapshotInstaller::new(
            node_id,
            replay_control,
        )),
    );
    let mut replayed_command = command;
    replayed_command.lease_id = uuid::Uuid::now_v7();
    let replayed = replay_executor.execute(replayed_command.clone()).await?;
    replayed.validate_against(&replayed_command)?;
    if replayed.outcome != recovered.outcome || replayed.completed_at != recovered.completed_at {
        return Err("completed Gateway command replay changed its durable outcome".into());
    }
    if tokio::fs::read_to_string(&apply_log).await? != "apply\nreplay\n" {
        return Err("completed Gateway command replay performed another native apply".into());
    }
    if replay_executor.journal().pending_acknowledgements().await? != vec![replayed.clone()] {
        return Err("recovered Gateway acknowledgement was not durably pending delivery".into());
    }
    let acknowledged_sequence = replay_executor
        .journal()
        .mark_acknowledged(NodeCommandAckReceipt {
            schema: NodeCommandAckReceipt::SCHEMA.into(),
            command_id: replayed.command_id,
            node_id: replayed.node_id,
            replayed: false,
        })
        .await?;
    if acknowledged_sequence != 1 || replay_executor.journal().after_sequence().await? != 1 {
        return Err("recovered Gateway acknowledgement did not advance the durable cursor".into());
    }
    drop(replay_executor);
    drop(gateway);

    let mut recovered_gateway = GatewayProcess::start(&binary, &config_path)?;
    wait_for_gateway(&base_url, &mut recovered_gateway.child).await?;
    let recovered_status = gateway_control(&base_url)?.readiness(&snapshot).await?;
    if recovered_status.state != ManagedSnapshotState::Applied || !recovered_status.ready {
        return Err("Gateway process restart did not recover the exact native snapshot".into());
    }
    wait_for_tcp_listener(traffic_port, &mut recovered_gateway.child).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "private subprocess used only by the Gateway reload crash gate"]
async fn gateway_apply_before_acknowledgement_crash_probe() -> TestResult {
    if required_probe_environment(CRASH_PROBE_PARENT_ENV)? != "1" {
        return Err("Gateway apply crash probe requires its private parent marker".into());
    }
    let base_url = required_probe_environment(CRASH_PROBE_BASE_URL_ENV)?;
    let state_dir = PathBuf::from(required_probe_environment(CRASH_PROBE_STATE_DIR_ENV)?);
    let command_path = PathBuf::from(required_probe_environment(CRASH_PROBE_COMMAND_ENV)?);
    let apply_log = PathBuf::from(required_probe_environment(CRASH_PROBE_LOG_ENV)?);
    let command: NodeCommandEnvelope =
        serde_json::from_slice(&tokio::fs::read(command_path).await?)?;
    command.validate()?;
    let control: Arc<dyn GatewayControl> = Arc::new(RecordedGatewayControl {
        inner: gateway_control(&base_url)?,
        apply_log,
        pause_after_apply: true,
    });
    let executor = CommandExecutor::new(
        FileCommandJournal::new(state_dir.clone(), command.node_id)?,
        Arc::new(UnusedRuntime),
        Arc::new(DurableGatewaySnapshotInstaller::new(
            command.node_id,
            control,
        )),
    );
    let result = executor.execute(command).await;
    Err(format!("Gateway apply crash probe returned without process death: {result:?}").into())
}

async fn wait_for_apply_crash_marker(
    apply_log: &Path,
    crash_probe: &mut CrashProbeProcess,
) -> TestResult {
    for _ in 0..100 {
        match tokio::fs::read_to_string(apply_log).await {
            Ok(contents) if contents == "apply\n" => return Ok(()),
            Ok(_) => return Err("Gateway crash probe wrote an invalid apply marker".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(status) = crash_probe.try_wait()? {
            return Err(format!(
                "Gateway crash probe exited before the apply boundary with {status}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Gateway crash probe did not reach the apply boundary".into())
}

async fn wait_for_tcp_listener(port: u16, child: &mut Child) -> TestResult {
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited after the crash probe reload".into());
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("A3S Gateway did not expose the reloaded traffic entrypoint".into())
}

fn required_gateway_binary() -> TestResult<String> {
    std::env::var("A3S_CLOUD_TEST_GATEWAY_BIN")
        .map_err(|_| "A3S_CLOUD_TEST_GATEWAY_BIN is required for remote Gateway tests".into())
}

fn required_probe_environment(name: &str) -> TestResult<String> {
    std::env::var(name).map_err(|_| {
        std::io::Error::other(format!("Gateway reload crash probe omitted {name}")).into()
    })
}

fn gateway_control(
    base_url: &str,
) -> Result<Arc<GatewayManagementClient>, GatewaySnapshotInstallError> {
    Ok(Arc::new(GatewayManagementClient::new(
        url::Url::parse(base_url)
            .map_err(|error| GatewaySnapshotInstallError::InvalidState(error.to_string()))?,
        GATEWAY_TOKEN.into(),
        Duration::from_secs(2),
        Duration::from_secs(2),
        Duration::from_secs(5),
    )?))
}

fn unused_ports() -> (u16, u16) {
    let traffic = TcpListener::bind("127.0.0.1:0").expect("bind traffic port");
    let management = TcpListener::bind("127.0.0.1:0").expect("bind management port");
    let ports = (
        traffic.local_addr().expect("traffic address").port(),
        management.local_addr().expect("management address").port(),
    );
    drop((traffic, management));
    ports
}

fn gateway_acl(
    traffic_port: u16,
    management_port: u16,
    gateway_id: uuid::Uuid,
    managed_state_file: &Path,
    revision: u64,
) -> String {
    format!(
        r#"# revision {revision}
entrypoints "web" {{ address = "127.0.0.1:{traffic_port}" }}

{}
"#,
        management_gateway_acl(management_port, gateway_id, managed_state_file)
    )
}

fn management_gateway_acl(
    management_port: u16,
    gateway_id: uuid::Uuid,
    managed_state_file: &Path,
) -> String {
    format!(
        r#"mode {{ kind = "cloud-managed" }}

managed {{
  gateway_id = "{gateway_id}"
  state_file = "{}"
}}

management {{
  enabled = true
  address = "127.0.0.1:{management_port}"
  path_prefix = "/api/gateway"
  auth_token_env = "A3S_GATEWAY_ADMIN_TOKEN"
  allowed_ips = ["127.0.0.1"]
}}"#,
        managed_state_file.display()
    )
}

async fn wait_for_gateway(base_url: &str, child: &mut Child) -> TestResult {
    let client = reqwest::Client::builder().no_proxy().build()?;
    for _ in 0..100 {
        if child.try_wait()?.is_some() {
            return Err("A3S Gateway exited before its management API was ready".into());
        }
        if client
            .get(format!("{base_url}/version"))
            .bearer_auth(GATEWAY_TOKEN)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("A3S Gateway management API did not become ready".into())
}
