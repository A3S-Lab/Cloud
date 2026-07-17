use super::health::host_port;
use super::{docker_error, is_status, DockerRuntimeDriver};
use a3s_cloud_contracts::RuntimeServiceEndpoint;
use a3s_runtime::contract::{
    HealthProbe, NetworkMode, RestartPolicy, RuntimeActionRequest, RuntimeEvidence, RuntimeFailure,
    RuntimeHealthObservation, RuntimeInspection, RuntimeMountSource, RuntimeObservation,
    RuntimeRemoval, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState, TransportProtocol,
};
use a3s_runtime::{RuntimeError, RuntimeResult, RuntimeUnitRecord};
use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::models::{
    ContainerInspectResponse, HealthConfig, HostConfig, PortBinding,
    RestartPolicy as DockerRestartPolicy, RestartPolicyNameEnum,
};
use chrono::DateTime;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MANAGED_LABEL: &str = "a3s.cloud.managed";
const NAMESPACE_LABEL: &str = "a3s.cloud.namespace";
const NODE_LABEL: &str = "a3s.cloud.node-id";
const UNIT_LABEL: &str = "a3s.runtime.unit-id";
const GENERATION_LABEL: &str = "a3s.runtime.generation";
const SPEC_DIGEST_LABEL: &str = "a3s.runtime.spec-digest";

impl DockerRuntimeDriver {
    pub(super) async fn apply_container(
        &self,
        spec: &RuntimeUnitSpec,
        _current: &RuntimeObservation,
    ) -> RuntimeResult<RuntimeObservation> {
        spec.validate().map_err(RuntimeError::InvalidRequest)?;
        let node_id = self.bound_node_id().await?;
        let spec_digest = spec.digest().map_err(RuntimeError::InvalidRequest)?;
        let provider_build = self.provider_build().await?;
        let image = self.ensure_image(&spec.artifact).await?;
        let mut container = self.find_container(node_id, spec, &spec_digest).await?;
        if container.is_none() {
            let name = container_name(&self.namespace, spec, &spec_digest);
            let config = self.container_config(node_id, spec, &spec_digest, image)?;
            match self
                .docker
                .create_container(
                    Some(CreateContainerOptions {
                        name,
                        platform: None,
                    }),
                    config,
                )
                .await
            {
                Ok(created) => {
                    container = Some(
                        self.docker
                            .inspect_container(&created.id, None)
                            .await
                            .map_err(docker_error)?,
                    );
                }
                Err(error) if is_status(&error, 409) => {
                    container = self.find_container(node_id, spec, &spec_digest).await?;
                    if container.is_none() {
                        return Err(RuntimeError::ProviderUnavailable(
                            "Docker container name conflicted without a matching managed resource"
                                .into(),
                        ));
                    }
                }
                Err(error) => return Err(docker_error(error)),
            }
        }
        let mut container = container.ok_or_else(|| {
            RuntimeError::Protocol("Docker apply lost the provider resource identity".into())
        })?;
        self.validate_container_binding(&container, node_id, spec, &spec_digest)?;
        if should_start(spec, &container) {
            let id = container_id(&container)?;
            match self
                .docker
                .start_container(&id, None::<StartContainerOptions<String>>)
                .await
            {
                Ok(()) => {}
                Err(error) if is_status(&error, 304) => {}
                Err(error) => return Err(docker_error(error)),
            }
            container = self
                .docker
                .inspect_container(&id, None)
                .await
                .map_err(docker_error)?;
        }
        let observation = match spec.class {
            RuntimeUnitClass::Task => self.wait_for_task(spec, container, &provider_build).await,
            RuntimeUnitClass::Service => {
                self.wait_for_service(spec, container, &provider_build)
                    .await
            }
        }?;
        let current_id = observation.provider_resource_id.as_deref().ok_or_else(|| {
            RuntimeError::Protocol("Docker apply observation has no resource identity".into())
        })?;
        self.retire_stale_containers(node_id, spec, current_id)
            .await?;
        Ok(observation)
    }

    pub(super) async fn inspect_unit(
        &self,
        unit: &RuntimeUnitRecord,
    ) -> RuntimeResult<RuntimeInspection> {
        let node_id = self.bound_node_id().await?;
        let digest = unit.spec.digest().map_err(RuntimeError::Protocol)?;
        let Some(container) = self.find_container(node_id, &unit.spec, &digest).await? else {
            return Ok(RuntimeInspection::NotFound {
                schema: RuntimeInspection::SCHEMA.into(),
                unit_id: unit.spec.unit_id.clone(),
                last_generation: Some(unit.spec.generation),
            });
        };
        let provider_build = self.provider_build().await?;
        let health = if container_is_running(&container) && unit.spec.health.is_some() {
            Some(self.probe_health(&unit.spec, &container).await?)
        } else {
            None
        };
        let observation = self.observation(&unit.spec, &container, &provider_build, health)?;
        Ok(RuntimeInspection::Found {
            schema: RuntimeInspection::SCHEMA.into(),
            observation: Box::new(observation),
        })
    }

    pub(super) async fn stop_unit(
        &self,
        unit: &RuntimeUnitRecord,
        _request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeObservation> {
        let node_id = self.bound_node_id().await?;
        let digest = unit.spec.digest().map_err(RuntimeError::Protocol)?;
        let provider_build = self.provider_build().await?;
        let Some(mut container) = self.find_container(node_id, &unit.spec, &digest).await? else {
            let mut unknown = unit.observation.clone();
            unknown.state = RuntimeUnitState::Unknown;
            unknown.observed_at_ms = now_ms();
            unknown.finished_at_ms = None;
            unknown.health = None;
            unknown.outputs.clear();
            unknown.failure = None;
            unknown.validate().map_err(RuntimeError::Protocol)?;
            return Ok(unknown);
        };
        let id = container_id(&container)?;
        if container_is_running(&container) {
            let seconds = self.operation_timeout.as_secs().min(i64::MAX as u64) as i64;
            match self
                .docker
                .stop_container(&id, Some(StopContainerOptions { t: seconds }))
                .await
            {
                Ok(()) => {}
                Err(error) if is_status(&error, 304) || is_status(&error, 404) => {}
                Err(error) => return Err(docker_error(error)),
            }
            container = self
                .docker
                .inspect_container(&id, None)
                .await
                .map_err(docker_error)?;
        }
        let mut observation = self.observation(&unit.spec, &container, &provider_build, None)?;
        observation.state = RuntimeUnitState::Stopped;
        observation.observed_at_ms = now_ms();
        observation.finished_at_ms = Some(observation.observed_at_ms);
        observation.health = None;
        observation.outputs.clear();
        observation.failure = None;
        observation
            .validate_against(&unit.spec)
            .map_err(RuntimeError::Protocol)?;
        Ok(observation)
    }

    pub(super) async fn remove_unit(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeRemoval> {
        let node_id = self.bound_node_id().await?;
        let digest = unit.spec.digest().map_err(RuntimeError::Protocol)?;
        let container = self.find_container(node_id, &unit.spec, &digest).await?;
        let already_absent = container.is_none();
        if let Some(container) = container {
            let id = container_id(&container)?;
            match self
                .docker
                .remove_container(
                    &id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                )
                .await
            {
                Ok(()) => {}
                Err(error) if is_status(&error, 404) => {}
                Err(error) => return Err(docker_error(error)),
            }
        }
        let removal = RuntimeRemoval {
            schema: RuntimeRemoval::SCHEMA.into(),
            request_id: request.request_id.clone(),
            unit_id: request.unit_id.clone(),
            generation: request.generation,
            removed_at_ms: now_ms(),
            already_absent,
        };
        removal.validate().map_err(RuntimeError::Protocol)?;
        Ok(removal)
    }

    pub(super) async fn find_container(
        &self,
        node_id: Uuid,
        spec: &RuntimeUnitSpec,
        spec_digest: &str,
    ) -> RuntimeResult<Option<ContainerInspectResponse>> {
        let labels = managed_labels(&self.namespace, node_id, spec, spec_digest);
        let mut filters = HashMap::new();
        filters.insert(
            "label".to_owned(),
            labels
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect(),
        );
        let summaries = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(docker_error)?;
        if summaries.len() > 1 {
            return Err(RuntimeError::Protocol(format!(
                "Docker has multiple managed containers for unit {:?} generation {}",
                spec.unit_id, spec.generation
            )));
        }
        let Some(summary) = summaries.into_iter().next() else {
            return Ok(None);
        };
        let id = summary.id.ok_or_else(|| {
            RuntimeError::Protocol("Docker container summary has no resource ID".into())
        })?;
        let inspect = self
            .docker
            .inspect_container(&id, None)
            .await
            .map_err(docker_error)?;
        self.validate_container_binding(&inspect, node_id, spec, spec_digest)?;
        Ok(Some(inspect))
    }

    async fn retire_stale_containers(
        &self,
        node_id: Uuid,
        spec: &RuntimeUnitSpec,
        current_id: &str,
    ) -> RuntimeResult<()> {
        let container_ids = self
            .managed_unit_container_ids(node_id, &spec.unit_id)
            .await?;
        if !container_ids.iter().any(|id| id == current_id) {
            return Err(RuntimeError::ProviderUnavailable(
                "Docker lost the current container during generation reconciliation".into(),
            ));
        }

        for id in container_ids.iter().filter(|id| id.as_str() != current_id) {
            let container = match self.docker.inspect_container(id, None).await {
                Ok(container) => container,
                Err(error) if is_status(&error, 404) => continue,
                Err(error) => return Err(docker_error(error)),
            };
            self.validate_managed_unit_binding(&container, node_id, &spec.unit_id)?;
            match self
                .docker
                .remove_container(
                    id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: false,
                        link: false,
                    }),
                )
                .await
            {
                Ok(()) => {}
                Err(error) if is_status(&error, 404) => {}
                Err(error) => return Err(docker_error(error)),
            }
        }

        let remaining = self
            .managed_unit_container_ids(node_id, &spec.unit_id)
            .await?;
        if remaining.len() != 1 || remaining[0] != current_id {
            return Err(RuntimeError::Protocol(format!(
                "Docker generation reconciliation for unit {:?} left resources {remaining:?}",
                spec.unit_id
            )));
        }
        Ok(())
    }

    async fn managed_unit_container_ids(
        &self,
        node_id: Uuid,
        unit_id: &str,
    ) -> RuntimeResult<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert(
            "label".to_owned(),
            managed_unit_labels(&self.namespace, node_id, unit_id)
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect(),
        );
        let summaries = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(docker_error)?;
        let mut ids = summaries
            .into_iter()
            .map(|summary| {
                summary.id.ok_or_else(|| {
                    RuntimeError::Protocol("Docker container summary has no resource ID".into())
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        ids.sort_unstable();
        Ok(ids)
    }

    fn validate_managed_unit_binding(
        &self,
        container: &ContainerInspectResponse,
        node_id: Uuid,
        unit_id: &str,
    ) -> RuntimeResult<()> {
        let labels = container
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or_else(|| {
                RuntimeError::Protocol("managed Docker container has no labels".into())
            })?;
        for (key, expected) in managed_unit_labels(&self.namespace, node_id, unit_id) {
            if labels.get(&key) != Some(&expected) {
                return Err(RuntimeError::Protocol(format!(
                    "managed Docker container label {key:?} does not match unit ownership"
                )));
            }
        }
        Ok(())
    }

    fn validate_container_binding(
        &self,
        container: &ContainerInspectResponse,
        node_id: Uuid,
        spec: &RuntimeUnitSpec,
        spec_digest: &str,
    ) -> RuntimeResult<()> {
        let labels = container
            .config
            .as_ref()
            .and_then(|config| config.labels.as_ref())
            .ok_or_else(|| {
                RuntimeError::Protocol("managed Docker container has no labels".into())
            })?;
        for (key, expected) in managed_labels(&self.namespace, node_id, spec, spec_digest) {
            if labels.get(&key) != Some(&expected) {
                return Err(RuntimeError::Protocol(format!(
                    "managed Docker container label {key:?} does not match durable identity"
                )));
            }
        }
        Ok(())
    }

    fn container_config(
        &self,
        node_id: Uuid,
        spec: &RuntimeUnitSpec,
        spec_digest: &str,
        image: String,
    ) -> RuntimeResult<Config<String>> {
        let labels = managed_labels(&self.namespace, node_id, spec, spec_digest);
        let mut exposed_ports = HashMap::new();
        let mut port_bindings = HashMap::new();
        for port in &spec.network.ports {
            let protocol = match port.protocol {
                TransportProtocol::Tcp => "tcp",
                TransportProtocol::Udp => "udp",
            };
            let key = format!("{}/{protocol}", port.container_port);
            exposed_ports.insert(key.clone(), HashMap::new());
            port_bindings.insert(
                key,
                Some(vec![PortBinding {
                    host_ip: Some("127.0.0.1".into()),
                    host_port: Some(String::new()),
                }]),
            );
        }
        let mut binds = Vec::new();
        let mut tmpfs = HashMap::new();
        for mount in &spec.mounts {
            match &mount.source {
                RuntimeMountSource::Volume { volume_id } => binds.push(format!(
                    "{}:{}:{}",
                    volume_name(&self.namespace, volume_id),
                    mount.target,
                    if mount.read_only { "ro" } else { "rw" }
                )),
                RuntimeMountSource::Tmpfs { size_bytes } => {
                    let access = if mount.read_only { "ro" } else { "rw" };
                    tmpfs.insert(
                        mount.target.clone(),
                        format!("{access},noexec,nosuid,nodev,size={size_bytes}"),
                    );
                }
                RuntimeMountSource::Artifact { .. } => {
                    return Err(RuntimeError::UnsupportedCapabilities(vec![
                        "mount_kind:Artifact".into(),
                    ]));
                }
            }
        }
        if !spec.secrets.is_empty() {
            return Err(RuntimeError::UnsupportedCapabilities(vec![
                "feature:SecretReferences".into(),
            ]));
        }
        let (entrypoint, command) = if spec.process.command.is_empty() {
            (
                None,
                (!spec.process.args.is_empty()).then(|| spec.process.args.clone()),
            )
        } else {
            (
                Some(spec.process.command.clone()),
                Some(spec.process.args.clone()),
            )
        };
        Ok(Config {
            image: Some(image),
            entrypoint,
            cmd: command,
            env: Some(
                spec.process
                    .environment
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect(),
            ),
            working_dir: spec.process.working_directory.clone(),
            labels: Some(labels),
            exposed_ports: (!exposed_ports.is_empty()).then_some(exposed_ports),
            healthcheck: Some(docker_health_config(spec)?),
            host_config: Some(HostConfig {
                nano_cpus: Some(to_i64(
                    spec.resources.cpu_millis.saturating_mul(1_000_000),
                    "CPU limit",
                )?),
                memory: Some(to_i64(spec.resources.memory_bytes, "memory limit")?),
                memory_swap: Some(to_i64(spec.resources.memory_bytes, "memory limit")?),
                pids_limit: Some(i64::from(spec.resources.pids)),
                network_mode: Some(match spec.network.mode {
                    NetworkMode::None => "none".into(),
                    NetworkMode::Outbound | NetworkMode::Service => "bridge".into(),
                }),
                port_bindings: (!port_bindings.is_empty()).then_some(port_bindings),
                publish_all_ports: Some(false),
                restart_policy: Some(restart_policy(&spec.restart)),
                binds: (!binds.is_empty()).then_some(binds),
                tmpfs: (!tmpfs.is_empty()).then_some(tmpfs),
                init: Some(true),
                privileged: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    pub(super) fn observation(
        &self,
        spec: &RuntimeUnitSpec,
        container: &ContainerInspectResponse,
        provider_build: &str,
        health: Option<RuntimeHealthObservation>,
    ) -> RuntimeResult<RuntimeObservation> {
        let state = container.state.as_ref().ok_or_else(|| {
            RuntimeError::Protocol("Docker container inspection has no state".into())
        })?;
        let running = state.running == Some(true);
        let exit_code = state.exit_code.unwrap_or_default();
        let (unit_state, failure) = if running {
            (RuntimeUnitState::Running, None)
        } else if state
            .status
            .as_ref()
            .is_some_and(|status| status.as_ref() == "created")
        {
            (RuntimeUnitState::Starting, None)
        } else if spec.class == RuntimeUnitClass::Task && exit_code == 0 {
            (RuntimeUnitState::Succeeded, None)
        } else if spec.class == RuntimeUnitClass::Service && exit_code == 0 {
            (RuntimeUnitState::Stopped, None)
        } else {
            (
                RuntimeUnitState::Failed,
                Some(RuntimeFailure {
                    code: "container_exit".into(),
                    message: container_failure_message(state.error.as_deref(), exit_code),
                    retryable: false,
                }),
            )
        };
        let observed_at = now_ms();
        let terminal = unit_state.is_terminal();
        let spec_digest = spec.digest().map_err(RuntimeError::Protocol)?;
        let observation = RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.into(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            spec_digest: spec_digest.clone(),
            class: spec.class,
            state: unit_state,
            provider_resource_id: Some(container_id(container)?),
            provider_build: Some(provider_build.into()),
            observed_at_ms: observed_at,
            started_at_ms: parse_timestamp_ms(state.started_at.as_deref()),
            finished_at_ms: terminal
                .then(|| parse_timestamp_ms(state.finished_at.as_deref()).unwrap_or(observed_at)),
            health: running.then_some(health).flatten(),
            outputs: Vec::new(),
            usage: None,
            evidence: Some(RuntimeEvidence {
                provider_build: provider_build.into(),
                spec_digest,
                semantics_profile_digest: spec.semantics_profile_digest.clone(),
                claims: service_endpoint_claims(spec, container)?,
            }),
            provider_attestation: None,
            failure,
        };
        observation
            .validate_against(spec)
            .map_err(RuntimeError::Protocol)?;
        Ok(observation)
    }
}

fn service_endpoint_claims(
    spec: &RuntimeUnitSpec,
    container: &ContainerInspectResponse,
) -> RuntimeResult<BTreeMap<String, String>> {
    if spec.class != RuntimeUnitClass::Service {
        return Ok(BTreeMap::new());
    }
    spec.network
        .ports
        .iter()
        .filter(|port| port.protocol == TransportProtocol::Tcp)
        .map(|port| {
            let endpoint = RuntimeServiceEndpoint::node_local_http(
                port.name.clone(),
                host_port(spec, container, &port.name)?,
            )
            .map_err(RuntimeError::Protocol)?;
            Ok((endpoint.claim_key(), endpoint.origin))
        })
        .collect()
}

fn managed_labels(
    namespace: &str,
    node_id: Uuid,
    spec: &RuntimeUnitSpec,
    digest: &str,
) -> HashMap<String, String> {
    let mut labels = managed_unit_labels(namespace, node_id, &spec.unit_id);
    labels.insert(GENERATION_LABEL.into(), spec.generation.to_string());
    labels.insert(SPEC_DIGEST_LABEL.into(), digest.into());
    labels
}

fn managed_unit_labels(
    namespace: &str,
    node_id: Uuid,
    unit_id: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (MANAGED_LABEL.into(), "true".into()),
        (NAMESPACE_LABEL.into(), namespace.into()),
        (NODE_LABEL.into(), node_id.to_string()),
        (UNIT_LABEL.into(), unit_id.into()),
    ])
}

fn container_name(namespace: &str, spec: &RuntimeUnitSpec, digest: &str) -> String {
    let unit = format!("{:x}", Sha256::digest(spec.unit_id.as_bytes()));
    let digest = digest.strip_prefix("sha256:").unwrap_or(digest);
    format!(
        "a3s-{namespace}-{}-g{}-{}",
        &unit[..12],
        spec.generation,
        &digest[..12]
    )
}

fn volume_name(namespace: &str, volume_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(volume_id.as_bytes()));
    format!("a3s-{namespace}-volume-{}", &digest[..16])
}

fn should_start(spec: &RuntimeUnitSpec, container: &ContainerInspectResponse) -> bool {
    if container_is_running(container) {
        return false;
    }
    let status = container
        .state
        .as_ref()
        .and_then(|state| state.status.as_ref())
        .map(AsRef::as_ref);
    status == Some("created") || spec.class == RuntimeUnitClass::Service
}

pub(super) fn container_is_running(container: &ContainerInspectResponse) -> bool {
    container
        .state
        .as_ref()
        .is_some_and(|state| state.running == Some(true))
}

pub(super) fn container_id(container: &ContainerInspectResponse) -> RuntimeResult<String> {
    container
        .id
        .as_ref()
        .filter(|id| !id.is_empty())
        .cloned()
        .ok_or_else(|| RuntimeError::Protocol("Docker container has no resource ID".into()))
}

fn docker_health_config(spec: &RuntimeUnitSpec) -> RuntimeResult<HealthConfig> {
    let Some(health) = &spec.health else {
        return Ok(HealthConfig {
            test: Some(vec!["NONE".into()]),
            ..Default::default()
        });
    };
    let HealthProbe::Command { command } = &health.probe else {
        return Ok(HealthConfig {
            test: Some(vec!["NONE".into()]),
            ..Default::default()
        });
    };
    let mut test = Vec::with_capacity(command.len() + 1);
    test.push("CMD".into());
    test.extend(command.iter().cloned());
    Ok(HealthConfig {
        test: Some(test),
        interval: Some(to_i64(
            health.interval_ms.saturating_mul(1_000_000),
            "health interval",
        )?),
        timeout: Some(to_i64(
            health.timeout_ms.saturating_mul(1_000_000),
            "health timeout",
        )?),
        retries: Some(i64::from(health.failure_threshold)),
        start_period: Some(to_i64(
            health.start_period_ms.saturating_mul(1_000_000),
            "health start period",
        )?),
        start_interval: None,
    })
}

fn restart_policy(policy: &RestartPolicy) -> DockerRestartPolicy {
    let (name, maximum_retry_count) = match policy {
        RestartPolicy::Never => (RestartPolicyNameEnum::NO, 0),
        RestartPolicy::OnFailure { max_retries } => {
            (RestartPolicyNameEnum::ON_FAILURE, i64::from(*max_retries))
        }
        RestartPolicy::Always => (RestartPolicyNameEnum::UNLESS_STOPPED, 0),
    };
    DockerRestartPolicy {
        name: Some(name),
        maximum_retry_count: Some(maximum_retry_count),
    }
}

fn to_i64(value: u64, label: &str) -> RuntimeResult<i64> {
    i64::try_from(value)
        .map_err(|_| RuntimeError::InvalidRequest(format!("{label} exceeds Docker bounds")))
}

fn container_failure_message(error: Option<&str>, exit_code: i64) -> String {
    let error = error.unwrap_or_default().replace(['\0', '\r', '\n'], " ");
    let error = error.trim();
    if error.is_empty() {
        format!("container exited with code {exit_code}")
    } else {
        format!("container exited with code {exit_code}: {error}")
            .chars()
            .take(4096)
            .collect()
    }
}

fn parse_timestamp_ms(value: Option<&str>) -> Option<u64> {
    let timestamp = DateTime::parse_from_rfc3339(value?)
        .ok()?
        .timestamp_millis();
    u64::try_from(timestamp).ok().filter(|value| *value > 0)
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, ResourceLimits, RuntimeNetworkSpec, RuntimeProcessSpec,
    };
    use std::collections::BTreeMap;

    #[test]
    fn container_identity_is_stable_and_does_not_embed_the_unit_id() {
        let spec = RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: "tenant/project/secret-shaped-name".into(),
            generation: 3,
            class: RuntimeUnitClass::Service,
            artifact: ArtifactRef {
                uri: format!("oci://registry.example/app@sha256:{}", "a".repeat(64)),
                digest: format!("sha256:{}", "a".repeat(64)),
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: RuntimeProcessSpec {
                command: Vec::new(),
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: Vec::new(),
            network: RuntimeNetworkSpec {
                mode: NetworkMode::None,
                ports: Vec::new(),
            },
            resources: ResourceLimits {
                cpu_millis: 100,
                memory_bytes: 32 * 1024 * 1024,
                pids: 32,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: None,
            },
            isolation: IsolationLevel::Container,
            health: None,
            restart: RestartPolicy::Never,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        };
        let digest = spec.digest().expect("spec digest");
        let name = container_name("cloud", &spec, &digest);
        assert_eq!(name, container_name("cloud", &spec, &digest));
        assert!(!name.contains("secret-shaped-name"));
        assert!(name.len() <= 63);
    }
}
