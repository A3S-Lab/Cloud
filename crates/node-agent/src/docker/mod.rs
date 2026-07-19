mod container;
mod health;
mod image;
mod logs;
mod secrets;

use crate::{DockerConfig, NodeRuntimeBinding, NodeSecretTransport};
use a3s_runtime::contract::{
    HealthCheckKind, IsolationLevel, MountKind, NetworkMode, ResourceControl, RuntimeActionRequest,
    RuntimeCapabilities, RuntimeExecRequest, RuntimeExecResult, RuntimeFeature, RuntimeInspection,
    RuntimeLogChunk, RuntimeLogQuery, RuntimeObservation, RuntimeRemoval, RuntimeUnitClass,
    RuntimeUnitSpec,
};
use a3s_runtime::{ProviderId, RuntimeDriver, RuntimeError, RuntimeResult, RuntimeUnitRecord};
use async_trait::async_trait;
use bollard::errors::Error as DockerError;
use bollard::{Docker, API_DEFAULT_VERSION};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

const OCI_IMAGE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const DOCKER_IMAGE_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";

pub struct DockerRuntimeDriver {
    provider_id: ProviderId,
    pub(super) docker: Docker,
    pub(super) namespace: String,
    pub(super) operation_timeout: Duration,
    pub(super) node_id: RwLock<Option<Uuid>>,
    pub(super) secret_transport: RwLock<Option<Arc<dyn NodeSecretTransport>>>,
    pub(super) secret_memory_dir: PathBuf,
    pub(super) health_client: reqwest::Client,
}

impl DockerRuntimeDriver {
    pub fn connect(config: &DockerConfig) -> RuntimeResult<Self> {
        let socket = config
            .socket
            .strip_prefix("unix://")
            .ok_or_else(|| RuntimeError::InvalidRequest("Docker socket must use unix://".into()))?;
        if !Path::new(socket).is_absolute() {
            return Err(RuntimeError::InvalidRequest(
                "Docker socket path must be absolute".into(),
            ));
        }
        if !crate::config::valid_docker_namespace(&config.namespace) {
            return Err(RuntimeError::InvalidRequest(
                "Docker namespace is invalid".into(),
            ));
        }
        if !config.secret_memory_dir.is_absolute()
            || config
                .secret_memory_dir
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(RuntimeError::InvalidRequest(
                "Docker Secret memory directory must be an absolute normalized path".into(),
            ));
        }
        let timeout_seconds = config.operation_timeout_ms.div_ceil(1_000).max(1);
        let docker = Docker::connect_with_unix(socket, timeout_seconds, API_DEFAULT_VERSION)
            .map_err(docker_error)?;
        let health_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .build()
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
        Ok(Self {
            provider_id: ProviderId::parse("docker")?,
            docker,
            namespace: config.namespace.clone(),
            operation_timeout: Duration::from_millis(config.operation_timeout_ms),
            node_id: RwLock::new(None),
            secret_transport: RwLock::new(None),
            secret_memory_dir: config.secret_memory_dir.join(&config.namespace),
            health_client,
        })
    }

    pub async fn bound_node_id(&self) -> RuntimeResult<Uuid> {
        self.node_id.read().await.as_ref().copied().ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "Docker Runtime is not bound to an enrolled Cloud node".into(),
            )
        })
    }

    pub(super) async fn provider_build(&self) -> RuntimeResult<String> {
        let version = self.docker.version().await.map_err(docker_error)?;
        let engine = version.version.unwrap_or_else(|| "unknown".into());
        let api = version.api_version.unwrap_or_else(|| "unknown".into());
        Ok(format!("docker/{engine} api/{api}"))
    }

    pub(super) async fn bounded<T, F>(&self, operation: &'static str, future: F) -> RuntimeResult<T>
    where
        F: std::future::Future<Output = RuntimeResult<T>>,
    {
        tokio::time::timeout(self.operation_timeout, future)
            .await
            .map_err(|_| {
                RuntimeError::ProviderUnavailable(format!(
                    "Docker {operation} exceeded the configured operation timeout"
                ))
            })?
    }
}

#[async_trait]
impl NodeRuntimeBinding for DockerRuntimeDriver {
    async fn bind_node(&self, node_id: Uuid) -> RuntimeResult<()> {
        if node_id.is_nil() {
            return Err(RuntimeError::InvalidRequest(
                "Docker Runtime node ID must not be nil".into(),
            ));
        }
        let mut current = self.node_id.write().await;
        match *current {
            Some(existing) if existing != node_id => Err(RuntimeError::RequestConflict {
                request_id: "docker-node-binding".into(),
            }),
            Some(_) => Ok(()),
            None => {
                *current = Some(node_id);
                Ok(())
            }
        }
    }

    async fn bind_secret_transport(
        &self,
        transport: Arc<dyn NodeSecretTransport>,
    ) -> RuntimeResult<()> {
        let mut current = self.secret_transport.write().await;
        match current.as_ref() {
            Some(existing) if !Arc::ptr_eq(existing, &transport) => {
                Err(RuntimeError::RequestConflict {
                    request_id: "docker-secret-transport-binding".into(),
                })
            }
            Some(_) => Ok(()),
            None => {
                *current = Some(transport);
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RuntimeDriver for DockerRuntimeDriver {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        let capabilities = RuntimeCapabilities {
            schema: RuntimeCapabilities::SCHEMA.into(),
            provider_id: self.provider_id.clone(),
            provider_build: self.provider_build().await?,
            unit_classes: vec![RuntimeUnitClass::Task, RuntimeUnitClass::Service],
            artifact_media_types: vec![OCI_IMAGE_MANIFEST.into(), DOCKER_IMAGE_MANIFEST.into()],
            isolation_levels: vec![IsolationLevel::Container],
            network_modes: vec![
                NetworkMode::None,
                NetworkMode::Outbound,
                NetworkMode::Service,
            ],
            mount_kinds: vec![MountKind::Volume, MountKind::Tmpfs],
            health_check_kinds: vec![
                HealthCheckKind::Http,
                HealthCheckKind::Tcp,
                HealthCheckKind::Command,
            ],
            resource_controls: vec![
                ResourceControl::Cpu,
                ResourceControl::Memory,
                ResourceControl::Pids,
                ResourceControl::ExecutionTimeout,
            ],
            features: vec![
                RuntimeFeature::DurableIdentity,
                RuntimeFeature::Stop,
                RuntimeFeature::Remove,
                RuntimeFeature::Logs,
                RuntimeFeature::SecretReferences,
            ],
        };
        capabilities.validate().map_err(RuntimeError::Protocol)?;
        Ok(capabilities)
    }

    async fn apply(
        &self,
        spec: &RuntimeUnitSpec,
        current: &RuntimeObservation,
    ) -> RuntimeResult<RuntimeObservation> {
        self.bounded("apply", self.apply_container(spec, current))
            .await
    }

    async fn inspect(&self, unit: &RuntimeUnitRecord) -> RuntimeResult<RuntimeInspection> {
        self.bounded("inspection", self.inspect_unit(unit)).await
    }

    async fn stop(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeObservation> {
        self.bounded("stop", self.stop_unit(unit, request)).await
    }

    async fn remove(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeRemoval> {
        self.bounded("remove", self.remove_unit(unit, request))
            .await
    }

    async fn logs(
        &self,
        unit: &RuntimeUnitRecord,
        query: &RuntimeLogQuery,
    ) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        self.bounded("log read", self.read_logs(unit, query)).await
    }

    async fn exec(
        &self,
        _unit: &RuntimeUnitRecord,
        _request: &RuntimeExecRequest,
    ) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::UnsupportedCapabilities(vec![
            "feature:Exec".into()
        ]))
    }
}

pub(super) fn docker_error(error: DockerError) -> RuntimeError {
    RuntimeError::ProviderUnavailable(sanitize_docker_error(&error.to_string()))
}

pub(super) fn is_status(error: &DockerError, expected: u16) -> bool {
    matches!(
        error,
        DockerError::DockerResponseServerError { status_code, .. } if *status_code == expected
    )
}

fn sanitize_docker_error(message: &str) -> String {
    let value = message.replace(['\0', '\r', '\n'], " ");
    let value = value.trim();
    if value.is_empty() {
        "Docker operation failed".into()
    } else {
        value.chars().take(16 * 1024).collect()
    }
}
