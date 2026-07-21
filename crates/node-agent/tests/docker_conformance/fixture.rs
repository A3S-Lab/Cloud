use super::{artifacts::DockerConformanceArtifacts, secrets::conformance_secret_transport, specs};
use a3s_cloud_node_agent::{
    DockerConfig, DockerRuntimeDriver, NodeArtifactManager, NodeRuntimeBinding,
};
use a3s_runtime::contract::{RuntimeCapabilities, RuntimeInspection, RuntimeObservation};
use a3s_runtime::{
    runtime_profile_requirements, FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient,
    RuntimeConformanceFixture, RuntimeConformanceInventory, RuntimeConformanceProfile,
    RuntimeConformanceProfileEvidence, RuntimeDriver, RuntimeError, RuntimeResult,
    RuntimeStateStore,
};
use async_trait::async_trait;
use bollard::container::{
    ListContainersOptions, RemoveContainerOptions, RestartContainerOptions, StopContainerOptions,
};
use bollard::errors::Error as DockerError;
use bollard::volume::{ListVolumesOptions, RemoveVolumeOptions};
use bollard::{Docker, API_DEFAULT_VERSION};
use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

const PROVIDER_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const NAMESPACE_LABEL: &str = "a3s.cloud.namespace";
const NODE_LABEL: &str = "a3s.cloud.node-id";
const RESTART_TARGET_LABEL: &str = "a3s.runtime.conformance.provider";
const UNIT_LABEL: &str = "a3s.runtime.unit-id";

pub(crate) struct DockerConformanceFixture {
    pub(crate) namespace: String,
    pub(crate) node_id: Uuid,
    pub(crate) driver: Arc<DockerRuntimeDriver>,
    pub(crate) store: Arc<FileRuntimeStateStore>,
    pub(crate) docker: Docker,
    pub(crate) artifacts: Arc<DockerConformanceArtifacts>,
    base: a3s_runtime::RuntimeBaseConformanceCase,
}

impl DockerConformanceFixture {
    pub(crate) fn new(
        namespace: String,
        node_id: Uuid,
        driver: Arc<DockerRuntimeDriver>,
        store: Arc<FileRuntimeStateStore>,
        artifacts: Arc<DockerConformanceArtifacts>,
    ) -> Self {
        Self {
            base: specs::base_case(&namespace),
            namespace,
            node_id,
            docker: connect_provider_docker().expect("connect Docker fixture client"),
            driver,
            store,
            artifacts,
        }
    }

    pub(crate) fn restarted_client(
        &self,
        driver: Arc<DockerRuntimeDriver>,
    ) -> ManagedRuntimeClient {
        ManagedRuntimeClient::new(self.store.clone() as Arc<dyn RuntimeStateStore>, driver)
    }

    /// Bypass managed terminal-state replay so reconstruction checks exercise
    /// the provider driver's inspection boundary.
    pub(crate) async fn inspect_driver(
        &self,
        driver: &dyn RuntimeDriver,
        unit_id: &str,
    ) -> RuntimeResult<RuntimeInspection> {
        let record = self.store.load(unit_id).await?;
        driver.inspect(&record).await
    }

    pub(crate) async fn namespace_container_ids(&self) -> RuntimeResult<Vec<String>> {
        self.container_ids(
            vec![format!("{NAMESPACE_LABEL}={}", self.namespace)],
            "list namespace containers",
        )
        .await
    }

    pub(crate) async fn namespace_volume_names(&self) -> RuntimeResult<Vec<String>> {
        let prefix = format!("a3s-{}-volume-", self.namespace);
        let filters = HashMap::from([("name".to_owned(), vec![prefix.clone()])]);
        let response = self
            .docker_call(
                "list namespace volumes",
                self.docker
                    .list_volumes(Some(ListVolumesOptions { filters })),
            )
            .await?;
        let mut names = response
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(|volume| volume.name)
            .filter(|name| name.starts_with(&prefix))
            .collect::<Vec<_>>();
        names.sort_unstable();
        Ok(names)
    }

    pub(crate) async fn unit_container_ids(&self, unit_id: &str) -> RuntimeResult<Vec<String>> {
        self.container_ids(
            vec![
                format!("{NAMESPACE_LABEL}={}", self.namespace),
                format!("{NODE_LABEL}={}", self.node_id),
                format!("{UNIT_LABEL}={unit_id}"),
            ],
            "list unit containers",
        )
        .await
    }

    async fn container_ids(
        &self,
        labels: Vec<String>,
        operation: &'static str,
    ) -> RuntimeResult<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert("label".to_owned(), labels);
        let containers = self
            .docker_call(
                operation,
                self.docker.list_containers(Some(ListContainersOptions {
                    all: true,
                    filters,
                    ..Default::default()
                })),
            )
            .await?;
        let mut ids = containers
            .into_iter()
            .map(|container| {
                container.id.ok_or_else(|| {
                    RuntimeError::Protocol("Docker container inventory omitted its ID".into())
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        ids.sort_unstable();
        Ok(ids)
    }

    pub(crate) async fn docker_call<T, F>(
        &self,
        operation: &'static str,
        future: F,
    ) -> RuntimeResult<T>
    where
        F: Future<Output = Result<T, DockerError>>,
    {
        tokio::time::timeout(PROVIDER_OPERATION_TIMEOUT, future)
            .await
            .map_err(|_| {
                RuntimeError::ProviderUnavailable(format!(
                    "Docker fixture {operation} exceeded 30 seconds"
                ))
            })?
            .map_err(|error| docker_fixture_error(operation, error))
    }

    pub(crate) async fn restart_provider(&self) -> RuntimeResult<()> {
        let target = std::env::var("A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER").map_err(|_| {
            RuntimeError::Protocol(
                "Recovery certification requires A3S_CLOUD_TEST_DOCKER_RESTART_CONTAINER for an isolated Docker daemon"
                    .into(),
            )
        })?;
        let control = Docker::connect_with_unix_defaults()
            .map_err(|error| docker_fixture_error("connect provider control daemon", error))?;
        let target_inspection = tokio::time::timeout(
            PROVIDER_OPERATION_TIMEOUT,
            control.inspect_container(&target, None),
        )
        .await
        .map_err(|_| {
            RuntimeError::ProviderUnavailable(
                "Docker provider restart target inspection timed out".into(),
            )
        })?
        .map_err(|error| docker_fixture_error("inspect provider restart target", error))?;
        let labels = target_inspection
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or_else(|| {
                RuntimeError::Protocol("Docker provider restart target has no labels".into())
            })?;
        require(
            labels.get(RESTART_TARGET_LABEL).map(String::as_str) == Some("true"),
            format!("Docker provider restart target must carry {RESTART_TARGET_LABEL}=true"),
        )?;
        let socket = docker_socket();
        let socket_path = Path::new(socket.strip_prefix("unix://").ok_or_else(|| {
            RuntimeError::InvalidRequest("Docker conformance socket must use unix://".into())
        })?);
        let owns_socket_directory = target_inspection.mounts.as_ref().is_some_and(|mounts| {
            mounts.iter().any(|mount| {
                mount.source.as_deref().is_some_and(|source| {
                    let source = Path::new(source);
                    source != socket_path && source.is_dir() && socket_path.starts_with(source)
                })
            })
        });
        require(
            owns_socket_directory,
            "Docker provider restart target does not own the configured socket directory",
        )?;

        tokio::time::timeout(
            PROVIDER_OPERATION_TIMEOUT,
            control.restart_container(&target, Some(RestartContainerOptions { t: 10 })),
        )
        .await
        .map_err(|_| {
            RuntimeError::ProviderUnavailable("isolated Docker provider restart timed out".into())
        })?
        .map_err(|error| docker_fixture_error("restart isolated provider", error))?;

        for _ in 0..60 {
            if matches!(
                tokio::time::timeout(Duration::from_secs(1), self.docker.version()).await,
                Ok(Ok(_))
            ) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(RuntimeError::ProviderUnavailable(
            "isolated Docker provider did not become ready after restart".into(),
        ))
    }

    pub(crate) fn secret_generation_directory(&self, spec_digest: &str) -> RuntimeResult<PathBuf> {
        let digest = spec_digest
            .strip_prefix("sha256:")
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| {
                RuntimeError::Protocol("Docker Secret conformance digest is invalid".into())
            })?;
        Ok(secret_memory_root().join(&self.namespace).join(digest))
    }

    async fn remove_fixture_container(&self, id: &str) -> RuntimeResult<()> {
        match tokio::time::timeout(
            PROVIDER_OPERATION_TIMEOUT,
            self.docker
                .stop_container(id, Some(StopContainerOptions { t: 1 })),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) if docker_status(&error, 304) || docker_status(&error, 404) => {}
            Ok(Err(error)) => {
                return Err(docker_fixture_error(
                    "stop conformance container before cleanup",
                    error,
                ))
            }
            Err(_) => {
                return Err(RuntimeError::ProviderUnavailable(
                    "Docker fixture stop before cleanup exceeded 30 seconds".into(),
                ))
            }
        }

        for _ in 0..40 {
            match tokio::time::timeout(
                Duration::from_secs(5),
                self.docker.remove_container(
                    id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                ),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) if docker_status(&error, 404) => return Ok(()),
                Ok(Err(error)) if docker_status(&error, 409) => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                Ok(Err(error)) => {
                    return Err(docker_fixture_error("remove conformance container", error))
                }
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
            }
            match tokio::time::timeout(
                Duration::from_secs(2),
                self.docker.inspect_container(id, None),
            )
            .await
            {
                Ok(Err(error)) if docker_status(&error, 404) => return Ok(()),
                Ok(Ok(_)) | Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
                Ok(Err(error)) => {
                    return Err(docker_fixture_error(
                        "verify conformance container cleanup",
                        error,
                    ))
                }
            }
        }
        Err(RuntimeError::ProviderUnavailable(format!(
            "Docker conformance container {id} did not disappear after cleanup"
        )))
    }

    async fn evidence(
        &self,
        capabilities: &RuntimeCapabilities,
        profile: RuntimeConformanceProfile,
    ) -> RuntimeResult<RuntimeConformanceProfileEvidence> {
        let requirements = runtime_profile_requirements(capabilities, profile)?;
        Ok(RuntimeConformanceProfileEvidence {
            profile,
            case_ids: requirements.case_ids,
            capability_claims: requirements.capability_claims,
        })
    }
}

#[async_trait]
impl RuntimeConformanceFixture for DockerConformanceFixture {
    fn base_case(&self) -> &a3s_runtime::RuntimeBaseConformanceCase {
        &self.base
    }

    fn available_profiles(&self) -> BTreeSet<RuntimeConformanceProfile> {
        BTreeSet::from([
            RuntimeConformanceProfile::Recovery,
            RuntimeConformanceProfile::Networking,
            RuntimeConformanceProfile::Mounts,
            RuntimeConformanceProfile::Health,
            RuntimeConformanceProfile::Resources,
            RuntimeConformanceProfile::Logs,
            RuntimeConformanceProfile::Security,
            RuntimeConformanceProfile::Outputs,
        ])
    }

    async fn inventory(&self) -> RuntimeResult<RuntimeConformanceInventory> {
        let mut inventory = RuntimeConformanceInventory::default();
        for id in self.namespace_container_ids().await? {
            let container = self
                .docker_call(
                    "inspect inventory container",
                    self.docker.inspect_container(&id, None),
                )
                .await?;
            let state = container
                .state
                .as_ref()
                .and_then(|state| state.status.as_ref())
                .map(|state| state.as_ref())
                .unwrap_or("unknown");
            let image = container.image.as_deref().unwrap_or("unknown");
            inventory
                .entries
                .insert(format!("container:{id}"), format!("{state}:{image}"));
        }
        for name in self.namespace_volume_names().await? {
            inventory
                .entries
                .insert(format!("volume:{name}"), "local".into());
        }
        Ok(inventory)
    }

    async fn run_profile(
        &self,
        client: &dyn RuntimeClient,
        capabilities: &RuntimeCapabilities,
        profile: RuntimeConformanceProfile,
    ) -> RuntimeResult<RuntimeConformanceProfileEvidence> {
        eprintln!("A3S_RUNTIME_PROFILE_START profile={}", profile.as_str());
        match profile {
            RuntimeConformanceProfile::Recovery => self.run_recovery(client).await?,
            RuntimeConformanceProfile::Networking => self.run_networking(client).await?,
            RuntimeConformanceProfile::Mounts => self.run_mounts(client).await?,
            RuntimeConformanceProfile::Health => self.run_health(client).await?,
            RuntimeConformanceProfile::Resources => self.run_resources(client).await?,
            RuntimeConformanceProfile::Logs => self.run_logs(client).await?,
            RuntimeConformanceProfile::Security => self.run_security(client).await?,
            RuntimeConformanceProfile::Outputs => self.run_outputs(client).await?,
            RuntimeConformanceProfile::Base
            | RuntimeConformanceProfile::Exec
            | RuntimeConformanceProfile::Evidence => {
                return Err(RuntimeError::Protocol(format!(
                    "Docker fixture was asked to run unexpected {} profile",
                    profile.as_str()
                )))
            }
        }
        let evidence = self.evidence(capabilities, profile).await?;
        eprintln!("A3S_RUNTIME_PROFILE_PASS profile={}", profile.as_str());
        Ok(evidence)
    }

    async fn cleanup(&self) -> RuntimeResult<()> {
        let mut failures = Vec::new();
        for id in self.namespace_container_ids().await? {
            let result = self.remove_fixture_container(&id).await;
            if let Err(error) = result {
                failures.push(error.to_string());
            }
        }
        for name in self.namespace_volume_names().await? {
            let result = self
                .docker_call(
                    "remove conformance volume",
                    self.docker
                        .remove_volume(&name, Some(RemoveVolumeOptions { force: true })),
                )
                .await;
            if let Err(error) = result {
                failures.push(error.to_string());
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(RuntimeError::ProviderUnavailable(format!(
                "Docker conformance cleanup failed: {}",
                failures.join("; ")
            )))
        }
    }
}

pub(crate) async fn connect_driver(
    namespace: &str,
    node_id: Uuid,
    artifacts: Arc<NodeArtifactManager>,
) -> RuntimeResult<DockerRuntimeDriver> {
    let driver = DockerRuntimeDriver::connect(&DockerConfig {
        socket: docker_socket(),
        namespace: namespace.into(),
        operation_timeout_ms: 30_000,
        secret_memory_dir: secret_memory_root(),
    })?;
    driver.bind_node(node_id).await?;
    driver
        .bind_secret_transport(conformance_secret_transport())
        .await?;
    driver.bind_artifact_manager(artifacts).await?;
    Ok(driver)
}

fn docker_socket() -> String {
    std::env::var("A3S_CLOUD_TEST_DOCKER_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".into())
}

fn secret_memory_root() -> PathBuf {
    std::env::var_os("A3S_CLOUD_TEST_SECRET_MEMORY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/dev/shm/a3s-cloud/test-secrets"))
}

fn connect_provider_docker() -> RuntimeResult<Docker> {
    let socket = docker_socket();
    let path = socket.strip_prefix("unix://").ok_or_else(|| {
        RuntimeError::InvalidRequest("Docker conformance socket must use unix://".into())
    })?;
    Docker::connect_with_unix(path, 30, API_DEFAULT_VERSION)
        .map_err(|error| docker_fixture_error("connect provider daemon", error))
}

pub(crate) fn require(condition: bool, message: impl Into<String>) -> RuntimeResult<()> {
    if condition {
        Ok(())
    } else {
        Err(RuntimeError::Protocol(message.into()))
    }
}

pub(crate) fn found(inspection: RuntimeInspection) -> RuntimeResult<RuntimeObservation> {
    match inspection {
        RuntimeInspection::Found { observation, .. } => Ok(*observation),
        RuntimeInspection::NotFound { unit_id, .. } => Err(RuntimeError::Protocol(format!(
            "Docker conformance unit {unit_id:?} unexpectedly disappeared"
        ))),
    }
}

pub(crate) fn resource_id(observation: &RuntimeObservation) -> RuntimeResult<&str> {
    observation
        .provider_resource_id
        .as_deref()
        .ok_or_else(|| RuntimeError::Protocol("Docker observation omitted resource ID".into()))
}

fn docker_fixture_error(operation: &str, error: DockerError) -> RuntimeError {
    RuntimeError::ProviderUnavailable(format!("Docker fixture {operation} failed: {error}"))
}

fn docker_status(error: &DockerError, expected: u16) -> bool {
    matches!(
        error,
        DockerError::DockerResponseServerError { status_code, .. }
            if *status_code == expected
    )
}
