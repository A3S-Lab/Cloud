use super::contract::{fixture_error, inventory_authority, inventory_for_binding};
use crate::artifacts::DockerConformanceArtifacts;
use crate::fixture::connect_driver;
use crate::resolve_artifact_state_root;
use a3s_cloud_contracts::{NodeCommandEnvelope, NodeCommandPayload};
use a3s_cloud_node_agent::{CommandExecutor, FileCommandJournal};
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeCapabilities, RuntimeExecRequest, RuntimeExecResult,
    RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation, RuntimeRemoval,
    RuntimeUnitSpec,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, ProviderId, RuntimeClient, RuntimeDriver,
    RuntimeError, RuntimeResult, RuntimeStateStore, RuntimeUnitRecord,
};
use async_trait::async_trait;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const CRASH_PROBE_TEST: &str = "resource_claims::resource_claim_provider_crash_probe";
const CRASH_PROBE_PARENT_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_PROBE";
const CRASH_PROBE_STATE_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_STATE_DIR";
const CRASH_PROBE_COMMAND_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_COMMAND";
const CRASH_PROBE_MARKER_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_MARKER";
const CRASH_PROBE_NAMESPACE_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_NAMESPACE";
const CRASH_PROBE_NODE_ENV: &str = "A3S_CLOUD_RESOURCE_CLAIM_CRASH_NODE_ID";

pub(super) async fn run_provider_crash_probe() -> RuntimeResult<()> {
    if required_environment(CRASH_PROBE_PARENT_ENV)? != "1" {
        return Err(RuntimeError::InvalidRequest(
            "resource Claim crash probe requires its private parent marker".into(),
        ));
    }
    let state_directory = PathBuf::from(required_environment(CRASH_PROBE_STATE_ENV)?);
    let command_path = PathBuf::from(required_environment(CRASH_PROBE_COMMAND_ENV)?);
    let marker_path = PathBuf::from(required_environment(CRASH_PROBE_MARKER_ENV)?);
    let namespace = required_environment(CRASH_PROBE_NAMESPACE_ENV)?;
    let node_id = Uuid::parse_str(&required_environment(CRASH_PROBE_NODE_ENV)?)
        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
    let command: NodeCommandEnvelope = serde_json::from_slice(
        &std::fs::read(&command_path)
            .map_err(|error| fixture_error("read resource Claim crash command", error))?,
    )
    .map_err(|error| {
        RuntimeError::Protocol(format!(
            "could not decode resource Claim crash command: {error}"
        ))
    })?;
    command.validate().map_err(RuntimeError::Protocol)?;
    let NodeCommandPayload::RuntimeApply {
        resource_claim: Some(binding),
        ..
    } = &command.payload
    else {
        return Err(RuntimeError::InvalidRequest(
            "resource Claim crash probe requires a bound Runtime apply".into(),
        ));
    };
    let inventory = inventory_for_binding(binding)?;
    let authority = inventory_authority(inventory);
    let artifact_state_root = resolve_artifact_state_root(&state_directory);
    let artifacts = Arc::new(DockerConformanceArtifacts::new(
        &artifact_state_root,
        node_id,
    )?);
    let inner = Arc::new(connect_driver(&namespace, node_id, artifacts.manager()).await?);
    let driver: Arc<dyn RuntimeDriver> = Arc::new(PauseAfterProviderApply { inner, marker_path });
    let runtime_store: Arc<dyn RuntimeStateStore> =
        Arc::new(FileRuntimeStateStore::new(state_directory.join("runtime")));
    let runtime: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(runtime_store, driver));
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(state_directory.join("journal"), node_id)
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?,
        runtime,
    )
    .with_artifacts(artifacts.manager())
    .with_resource_inventory(authority);
    let result = executor.execute(command).await;
    Err(RuntimeError::Protocol(format!(
        "resource Claim crash probe returned before process death: {result:?}"
    )))
}

struct PauseAfterProviderApply {
    inner: Arc<a3s_cloud_node_agent::DockerRuntimeDriver>,
    marker_path: PathBuf,
}

#[async_trait]
impl RuntimeDriver for PauseAfterProviderApply {
    fn provider_id(&self) -> &ProviderId {
        self.inner.provider_id()
    }

    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        self.inner.capabilities().await
    }

    async fn apply(
        &self,
        spec: &RuntimeUnitSpec,
        current: &RuntimeObservation,
    ) -> RuntimeResult<RuntimeObservation> {
        let observation = self.inner.apply(spec, current).await?;
        let marker_path = self.marker_path.clone();
        let body = serde_json::to_vec(&observation).map_err(|error| {
            RuntimeError::Protocol(format!(
                "could not encode resource Claim provider marker: {error}"
            ))
        })?;
        tokio::task::spawn_blocking(move || write_durable_file(&marker_path, &body))
            .await
            .map_err(|error| {
                RuntimeError::ProviderUnavailable(format!(
                    "resource Claim provider marker task failed: {error}"
                ))
            })?
            .map_err(|error| fixture_error("persist resource Claim provider marker", error))?;
        std::future::pending::<RuntimeResult<RuntimeObservation>>().await
    }

    async fn inspect(&self, unit: &RuntimeUnitRecord) -> RuntimeResult<RuntimeInspection> {
        self.inner.inspect(unit).await
    }

    async fn stop(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeObservation> {
        self.inner.stop(unit, request).await
    }

    async fn remove(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeRemoval> {
        self.inner.remove(unit, request).await
    }

    async fn logs(
        &self,
        unit: &RuntimeUnitRecord,
        query: &RuntimeLogQuery,
    ) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        self.inner.logs(unit, query).await
    }

    async fn exec(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeExecRequest,
    ) -> RuntimeResult<RuntimeExecResult> {
        self.inner.exec(unit, request).await
    }
}

pub(super) struct ResourceClaimCrashProbe {
    child: Option<Child>,
}

impl ResourceClaimCrashProbe {
    pub(super) fn start(
        test_binary: &Path,
        state_directory: &Path,
        command_path: &Path,
        marker_path: &Path,
        namespace: &str,
        node_id: Uuid,
    ) -> RuntimeResult<Self> {
        let child = Command::new(test_binary)
            .arg(CRASH_PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_PROBE_PARENT_ENV, "1")
            .env(CRASH_PROBE_STATE_ENV, state_directory)
            .env(CRASH_PROBE_COMMAND_ENV, command_path)
            .env(CRASH_PROBE_MARKER_ENV, marker_path)
            .env(CRASH_PROBE_NAMESPACE_ENV, namespace)
            .env(CRASH_PROBE_NODE_ENV, node_id.to_string())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| fixture_error("start resource Claim crash probe", error))?;
        Ok(Self { child: Some(child) })
    }

    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("resource Claim crash probe disappeared"))?
            .try_wait()
    }

    pub(super) fn kill_and_wait(mut self) -> RuntimeResult<ExitStatus> {
        let mut child = self.child.take().ok_or_else(|| {
            RuntimeError::Protocol("resource Claim crash probe disappeared".into())
        })?;
        if let Some(status) = child
            .try_wait()
            .map_err(|error| fixture_error("inspect resource Claim crash probe", error))?
        {
            return Ok(status);
        }
        child
            .kill()
            .map_err(|error| fixture_error("kill resource Claim crash probe", error))?;
        child
            .wait()
            .map_err(|error| fixture_error("wait for resource Claim crash probe", error))
    }
}

impl Drop for ResourceClaimCrashProbe {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(super) async fn wait_for_provider_apply_marker(
    marker_path: &Path,
    crash_probe: &mut ResourceClaimCrashProbe,
) -> RuntimeResult<RuntimeObservation> {
    for _ in 0..1_200 {
        match std::fs::read(marker_path) {
            Ok(body) => {
                return serde_json::from_slice(&body).map_err(|error| {
                    RuntimeError::Protocol(format!(
                        "could not decode resource Claim provider marker: {error}"
                    ))
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(fixture_error("read resource Claim provider marker", error)),
        }
        if let Some(status) = crash_probe
            .try_wait()
            .map_err(|error| fixture_error("inspect resource Claim crash probe", error))?
        {
            return Err(RuntimeError::ProviderUnavailable(format!(
                "resource Claim crash probe exited before provider apply with {status}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err(RuntimeError::ProviderUnavailable(
        "resource Claim crash probe did not complete provider apply within 60 seconds".into(),
    ))
}

pub(super) fn write_durable_file(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("durable resource Claim file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("durable resource Claim file name is invalid"))?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
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

fn required_environment(name: &str) -> RuntimeResult<String> {
    std::env::var(name)
        .map_err(|_| RuntimeError::InvalidRequest(format!("resource Claim probe omitted {name}")))
}
