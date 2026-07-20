use crate::deployment_flow_support::PostgresSecretTransport;
use a3s_cloud_control_plane::modules::shared_kernel::domain::{NodeId, OrganizationId};
use a3s_cloud_control_plane::modules::workloads::{
    IWorkloadRepository, PostgresWorkloadRepository,
};
use a3s_cloud_node_agent::{
    DockerConfig, DockerRuntimeDriver, NodeRuntimeBinding, NodeSecretTransport,
};
use a3s_orm::PostgresExecutor;
use a3s_runtime::contract::{
    RuntimeActionRequest, RuntimeApplyRequest, RuntimeCapabilities, RuntimeExecRequest,
    RuntimeExecResult, RuntimeInspection, RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation,
    RuntimeRemoval, RuntimeUnitSpec,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, ProviderId, RuntimeDriver, RuntimeError,
    RuntimeResult, RuntimeStateStore, RuntimeUnitRecord,
};
use async_trait::async_trait;
use bollard::container::{ListContainersOptions, RestartContainerOptions};
use bollard::{Docker, API_DEFAULT_VERSION};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const CRASH_PROBE_TEST: &str = "secret_rotation_provider_crash_probe";
const CRASH_PROBE_PARENT_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_PROBE";
const CRASH_PROBE_POSTGRES_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_POSTGRES_URL";
const CRASH_PROBE_ORGANIZATION_ENV: &str =
    "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_ORGANIZATION_ID";
const CRASH_PROBE_NODE_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_NODE_ID";
const CRASH_PROBE_STATE_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_STATE_DIR";
const CRASH_PROBE_REQUEST_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_REQUEST";
const CRASH_PROBE_MARKER_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_MARKER";
const CRASH_PROBE_SECURITY_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_SECURITY_DIR";
const CRASH_PROBE_NAMESPACE_ENV: &str = "A3S_CLOUD_SECRET_ROTATION_PROVIDER_CRASH_NAMESPACE";
const MANAGED_LABEL: &str = "a3s.cloud.managed";
const NAMESPACE_LABEL: &str = "a3s.cloud.namespace";
const NODE_LABEL: &str = "a3s.cloud.node-id";
const PROVIDER_RESTART_LABEL: &str = "a3s.runtime.conformance.provider";
const UNIT_LABEL: &str = "a3s.runtime.unit-id";
const GENERATION_LABEL: &str = "a3s.runtime.generation";

pub(super) async fn run_provider_crash_probe() -> TestResult {
    if required_probe_environment(CRASH_PROBE_PARENT_ENV)? != "1" {
        return Err("Secret-rotation provider crash probe requires its private marker".into());
    }
    let postgres_url = required_probe_environment(CRASH_PROBE_POSTGRES_ENV)?;
    let organization_id = OrganizationId::from_uuid(Uuid::parse_str(&required_probe_environment(
        CRASH_PROBE_ORGANIZATION_ENV,
    )?)?);
    let node_id = NodeId::from_uuid(Uuid::parse_str(&required_probe_environment(
        CRASH_PROBE_NODE_ENV,
    )?)?);
    let state_directory = PathBuf::from(required_probe_environment(CRASH_PROBE_STATE_ENV)?);
    let request_path = PathBuf::from(required_probe_environment(CRASH_PROBE_REQUEST_ENV)?);
    let marker_path = PathBuf::from(required_probe_environment(CRASH_PROBE_MARKER_ENV)?);
    let security_state_dir = PathBuf::from(required_probe_environment(CRASH_PROBE_SECURITY_ENV)?);
    let namespace = required_probe_environment(CRASH_PROBE_NAMESPACE_ENV)?;
    let request: RuntimeApplyRequest = serde_json::from_slice(&std::fs::read(request_path)?)?;
    request.validate()?;

    let executor = PostgresExecutor::connect_no_tls(&postgres_url, 2)?;
    let inner = bound_docker_driver(
        &executor,
        organization_id,
        node_id,
        &security_state_dir,
        &namespace,
    )
    .await?;
    let driver: Arc<dyn RuntimeDriver> = Arc::new(PauseAfterProviderApply { inner, marker_path });
    let state: Arc<dyn RuntimeStateStore> =
        Arc::new(FileRuntimeStateStore::new(state_directory.join("runtime")));
    let runtime = ManagedRuntimeClient::new(state, driver);
    let result = a3s_runtime::RuntimeClient::apply(&runtime, &request).await;
    Err(
        format!("Secret-rotation provider crash probe returned before process death: {result:?}")
            .into(),
    )
}

struct PauseAfterProviderApply {
    inner: Arc<DockerRuntimeDriver>,
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
                "could not serialize Secret-rotation provider crash marker: {error}"
            ))
        })?;
        tokio::task::spawn_blocking(move || write_durable_file(&marker_path, &body))
            .await
            .map_err(|error| {
                RuntimeError::ProviderUnavailable(format!(
                    "Secret-rotation provider crash marker task failed: {error}"
                ))
            })?
            .map_err(|error| {
                RuntimeError::ProviderUnavailable(format!(
                    "could not persist Secret-rotation provider crash marker: {error}"
                ))
            })?;
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

pub(super) struct CrashProbeProcess {
    child: Option<Child>,
}

impl CrashProbeProcess {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start(
        test_binary: &Path,
        postgres_url: &str,
        organization_id: OrganizationId,
        node_id: NodeId,
        state_directory: &Path,
        request_path: &Path,
        marker_path: &Path,
        security_state_dir: &Path,
        namespace: &str,
    ) -> std::io::Result<Self> {
        let child = Command::new(test_binary)
            .arg(CRASH_PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CRASH_PROBE_PARENT_ENV, "1")
            .env(CRASH_PROBE_POSTGRES_ENV, postgres_url)
            .env(CRASH_PROBE_ORGANIZATION_ENV, organization_id.to_string())
            .env(CRASH_PROBE_NODE_ENV, node_id.to_string())
            .env(CRASH_PROBE_STATE_ENV, state_directory)
            .env(CRASH_PROBE_REQUEST_ENV, request_path)
            .env(CRASH_PROBE_MARKER_ENV, marker_path)
            .env(CRASH_PROBE_SECURITY_ENV, security_state_dir)
            .env(CRASH_PROBE_NAMESPACE_ENV, namespace)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| std::io::Error::other("provider crash probe process disappeared"))?
            .try_wait()
    }

    pub(super) fn kill_and_wait(mut self) -> std::io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| std::io::Error::other("provider crash probe process disappeared"))?;
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

pub(super) async fn bound_docker_driver(
    executor: &PostgresExecutor,
    organization_id: OrganizationId,
    node_id: NodeId,
    security_state_dir: &Path,
    namespace: &str,
) -> TestResult<Arc<DockerRuntimeDriver>> {
    let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
        socket: docker_socket(),
        namespace: namespace.into(),
        operation_timeout_ms: 30_000,
        secret_memory_dir: docker_secret_memory_dir(),
    })?);
    driver.bind_node(node_id.as_uuid()).await?;
    let workloads: Arc<dyn IWorkloadRepository> =
        Arc::new(PostgresWorkloadRepository::new(executor.clone()));
    let transport: Arc<dyn NodeSecretTransport> = Arc::new(PostgresSecretTransport::new(
        executor,
        workloads,
        organization_id,
        node_id,
        security_state_dir,
    )?);
    driver.bind_secret_transport(transport).await?;
    Ok(driver)
}

pub(super) async fn wait_for_provider_apply_marker(
    marker_path: &Path,
    crash_probe: &mut CrashProbeProcess,
) -> TestResult<RuntimeObservation> {
    for _ in 0..600 {
        match std::fs::read(marker_path) {
            Ok(body) => return Ok(serde_json::from_slice(&body)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(status) = crash_probe.try_wait()? {
            return Err(format!(
                "Secret-rotation provider crash probe exited before apply completed with {status}"
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Err("Secret-rotation provider crash probe did not complete its provider apply".into())
}

pub(super) async fn require_exact_container(
    provider_socket: &str,
    namespace: &str,
    node_id: NodeId,
    spec: &RuntimeUnitSpec,
    expected_id: &str,
) -> TestResult {
    let ids = managed_container_ids(provider_socket, namespace, node_id, spec).await?;
    if ids != [expected_id] {
        return Err(format!(
            "Secret-rotation provider recovery expected container {expected_id:?}, found {ids:?}"
        )
        .into());
    }
    Ok(())
}

pub(super) async fn managed_container_ids(
    provider_socket: &str,
    namespace: &str,
    node_id: NodeId,
    spec: &RuntimeUnitSpec,
) -> TestResult<Vec<String>> {
    let docker = connect_provider(provider_socket)?;
    let filters = HashMap::from([(
        "label".to_owned(),
        vec![
            format!("{MANAGED_LABEL}=true"),
            format!("{NAMESPACE_LABEL}={namespace}"),
            format!("{NODE_LABEL}={node_id}"),
            format!("{UNIT_LABEL}={}", spec.unit_id),
            format!("{GENERATION_LABEL}={}", spec.generation),
        ],
    )]);
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        }))
        .await?;
    let mut ids = containers
        .into_iter()
        .map(|container| {
            container.id.ok_or_else(|| {
                std::io::Error::other("managed Docker container omitted its identity")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort_unstable();
    Ok(ids)
}

pub(super) async fn restart_isolated_provider(provider_socket: &str) -> TestResult {
    let target = std::env::var("A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER")
        .map_err(|_| "isolated provider restart target is required")?;
    let control = Docker::connect_with_unix_defaults()?;
    let before = tokio::time::timeout(
        Duration::from_secs(30),
        control.inspect_container(&target, None),
    )
    .await??;
    let labels = before
        .config
        .as_ref()
        .and_then(|config| config.labels.as_ref())
        .ok_or("isolated provider restart target has no labels")?;
    if labels.get(PROVIDER_RESTART_LABEL).map(String::as_str) != Some("true") {
        return Err(format!(
            "isolated provider restart target lacks {PROVIDER_RESTART_LABEL}=true"
        )
        .into());
    }
    let socket_path = provider_socket
        .strip_prefix("unix://")
        .map(Path::new)
        .ok_or("isolated provider socket must use unix://")?;
    let owns_socket_directory = before.mounts.as_ref().is_some_and(|mounts| {
        mounts.iter().any(|mount| {
            mount.source.as_deref().is_some_and(|source| {
                let source = Path::new(source);
                source != socket_path && source.is_dir() && socket_path.starts_with(source)
            })
        })
    });
    if !owns_socket_directory {
        return Err("isolated provider restart target does not own the configured socket".into());
    }
    let before_pid = before
        .state
        .as_ref()
        .and_then(|state| state.pid)
        .filter(|pid| *pid > 0)
        .ok_or("isolated provider restart target has no live process")?;

    tokio::time::timeout(
        Duration::from_secs(60),
        control.restart_container(&target, Some(RestartContainerOptions { t: 10 })),
    )
    .await??;
    let provider = connect_provider(provider_socket)?;
    for _ in 0..120 {
        if matches!(
            tokio::time::timeout(Duration::from_secs(1), provider.version()).await,
            Ok(Ok(_))
        ) {
            let after = control.inspect_container(&target, None).await?;
            let after_pid = after
                .state
                .as_ref()
                .and_then(|state| state.pid)
                .filter(|pid| *pid > 0)
                .ok_or("restarted isolated provider has no live process")?;
            if after_pid == before_pid {
                return Err("isolated provider restart did not replace its process".into());
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("isolated provider did not become ready after process death".into())
}

fn connect_provider(socket: &str) -> TestResult<Docker> {
    let socket = socket
        .strip_prefix("unix://")
        .ok_or("Docker provider socket must use unix://")?;
    Ok(Docker::connect_with_unix(socket, 30, API_DEFAULT_VERSION)?)
}

pub(super) fn docker_socket() -> String {
    std::env::var("A3S_CLOUD_TEST_DOCKER_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".into())
}

pub(super) fn docker_secret_memory_dir() -> PathBuf {
    std::env::var("A3S_CLOUD_TEST_SECRET_MEMORY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("a3s-cloud-secrets"))
}

fn required_probe_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("provider crash probe omitted {name}")))
}

pub(super) fn write_durable_file(path: &Path, body: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("durable provider crash file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("durable provider crash file name is invalid"))?;
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
