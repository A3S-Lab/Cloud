use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use a3s_runtime::contract::{
    ArtifactRef, HealthCheckKind, IsolationLevel, MountKind, NetworkMode, ResourceControl,
    RuntimeActionRequest, RuntimeCapabilities, RuntimeEvidence, RuntimeExecRequest,
    RuntimeExecResult, RuntimeFailure, RuntimeFeature, RuntimeInspection, RuntimeLogChunk,
    RuntimeLogQuery, RuntimeLogStream, RuntimeObservation, RuntimeOutputArtifact, RuntimeRemoval,
    RuntimeUnitClass, RuntimeUnitState, RuntimeUsage, SecretTarget,
};
use a3s_runtime::{ProviderId, RuntimeDriver, RuntimeError, RuntimeResult, RuntimeUnitRecord};
use async_trait::async_trait;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::ProviderConfig;

type RuntimeLogStore = BTreeMap<(String, u64), Vec<RuntimeLogChunk>>;

struct TerminalOutcome {
    state: RuntimeUnitState,
    outputs: Vec<RuntimeOutputArtifact>,
    failure: Option<RuntimeFailure>,
}

#[derive(Clone)]
pub struct ProcessRuntimeDriver {
    provider_id: ProviderId,
    config: Arc<ProviderConfig>,
    client: Client,
    logs: Arc<RwLock<RuntimeLogStore>>,
}

impl ProcessRuntimeDriver {
    pub fn new(config: ProviderConfig) -> RuntimeResult<Self> {
        let provider_id = ProviderId::parse(config.id.clone())?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        Ok(Self {
            provider_id,
            config: Arc::new(config),
            client,
            logs: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    pub fn artifact_path(&self, digest_hex: &str) -> PathBuf {
        self.config.artifact_path.join(digest_hex)
    }

    async fn resolve_runner(&self, artifact: &ArtifactRef) -> RuntimeResult<PathBuf> {
        let url = url::Url::parse(&artifact.uri)
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        if url.scheme() != "file" {
            return Err(RuntimeError::UnsupportedCapabilities(vec![
                "dev provider requires a file:// primary artifact".to_string(),
            ]));
        }
        let path = url.to_file_path().map_err(|_| {
            RuntimeError::InvalidRequest("invalid file:// runner artifact".to_string())
        })?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        verify_digest(&bytes, &artifact.digest)?;
        Ok(path)
    }

    async fn materialize_mounts(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        root: &Path,
    ) -> RuntimeResult<BTreeMap<String, PathBuf>> {
        let mut mapped = BTreeMap::new();
        for mount in &spec.mounts {
            let a3s_runtime::contract::RuntimeMountSource::Artifact { artifact } = &mount.source
            else {
                return Err(RuntimeError::UnsupportedCapabilities(vec![format!(
                    "mount:{:?}",
                    mount.source.kind()
                )]));
            };
            let url = url::Url::parse(&artifact.uri)
                .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err(RuntimeError::UnsupportedCapabilities(vec![
                    "dev provider artifact mounts require http(s)".to_string(),
                ]));
            }
            let response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            if !response.status().is_success() {
                return Err(RuntimeError::Transport(format!(
                    "input artifact returned {}",
                    response.status()
                )));
            }
            if response
                .content_length()
                .is_some_and(|length| length > self.config.max_input_bytes)
            {
                return Err(RuntimeError::InvalidRequest(
                    "input artifact exceeds provider limit".to_string(),
                ));
            }
            let bytes = response
                .bytes()
                .await
                .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            if bytes.len() as u64 > self.config.max_input_bytes {
                return Err(RuntimeError::InvalidRequest(
                    "input artifact exceeds provider limit".to_string(),
                ));
            }
            verify_digest(&bytes, &artifact.digest)?;
            let path = sandbox_path(root, &mount.target)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            }
            tokio::fs::write(&path, &bytes)
                .await
                .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            mapped.insert(mount.target.clone(), path);
        }
        Ok(mapped)
    }

    async fn materialize_secrets(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        root: &Path,
        environment: &mut BTreeMap<String, String>,
    ) -> RuntimeResult<()> {
        for secret in &spec.secrets {
            let variable = secret.reference.strip_prefix("env://").ok_or_else(|| {
                RuntimeError::UnsupportedCapabilities(vec![
                    "dev provider only resolves env:// secret references".to_string(),
                ])
            })?;
            let value = std::env::var(variable).map_err(|_| {
                RuntimeError::ProviderUnavailable(format!(
                    "secret reference {:?} is unavailable",
                    secret.reference
                ))
            })?;
            match &secret.target {
                SecretTarget::Environment { variable } => {
                    environment.insert(variable.clone(), value);
                }
                SecretTarget::File { path, .. } => {
                    let path = sandbox_path(root, path)?;
                    if let Some(parent) = path.parent() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
                    }
                    tokio::fs::write(path, value)
                        .await
                        .map_err(|error| RuntimeError::Transport(error.to_string()))?;
                }
            }
        }
        Ok(())
    }

    async fn persist_outputs(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        root: &Path,
    ) -> RuntimeResult<Vec<RuntimeOutputArtifact>> {
        tokio::fs::create_dir_all(&self.config.artifact_path)
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        let mut outputs = Vec::new();
        for expected in &spec.outputs {
            let path = sandbox_path(root, &expected.path)?;
            let bytes = tokio::fs::read(&path).await.map_err(|error| {
                RuntimeError::Protocol(format!(
                    "task omitted output {} at {}: {error}",
                    expected.name,
                    path.display()
                ))
            })?;
            if bytes.len() as u64 > expected.max_bytes {
                return Err(RuntimeError::Protocol(format!(
                    "task output {} exceeds limit",
                    expected.name
                )));
            }
            let digest = digest(&bytes);
            let hex = digest.trim_start_matches("sha256:");
            let stored = self.artifact_path(hex);
            if tokio::fs::metadata(&stored).await.is_err() {
                tokio::fs::write(&stored, &bytes)
                    .await
                    .map_err(|error| RuntimeError::Transport(error.to_string()))?;
            }
            outputs.push(RuntimeOutputArtifact {
                name: expected.name.clone(),
                artifact: ArtifactRef {
                    uri: format!(
                        "{}/v1/artifacts/{hex}",
                        self.config.public_base_url.trim_end_matches('/')
                    ),
                    digest,
                    media_type: expected.media_type.clone(),
                },
                size_bytes: bytes.len() as u64,
            });
        }
        Ok(outputs)
    }

    async fn record_logs(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        stdout: &[u8],
        stderr: &[u8],
    ) {
        let observed_at_ms = now_ms();
        let mut chunks = Vec::new();
        for (sequence, (stream, bytes)) in [
            (RuntimeLogStream::Stdout, stdout),
            (RuntimeLogStream::Stderr, stderr),
        ]
        .into_iter()
        .enumerate()
        {
            if bytes.is_empty() {
                continue;
            }
            chunks.push(RuntimeLogChunk {
                schema: RuntimeLogChunk::SCHEMA.to_string(),
                cursor: format!("{}-{}", spec.generation, sequence + 1),
                sequence: (sequence + 1) as u64,
                observed_at_ms,
                stream,
                data: String::from_utf8_lossy(bytes).into_owned(),
            });
        }
        self.logs
            .write()
            .await
            .insert((spec.unit_id.clone(), spec.generation), chunks);
    }

    fn terminal_observation(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        resource_id: String,
        started_at_ms: u64,
        wall_time_ms: u64,
        outcome: TerminalOutcome,
    ) -> RuntimeResult<RuntimeObservation> {
        let spec_digest = spec.digest().map_err(RuntimeError::Protocol)?;
        Ok(RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.to_string(),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            spec_digest: spec_digest.clone(),
            class: spec.class,
            state: outcome.state,
            provider_resource_id: Some(resource_id),
            provider_build: Some(self.config.build.clone()),
            observed_at_ms: now_ms(),
            started_at_ms: Some(started_at_ms),
            finished_at_ms: Some(now_ms()),
            health: None,
            outputs: outcome.outputs,
            usage: Some(RuntimeUsage {
                wall_time_ms,
                cpu_time_ms: 0,
                peak_memory_bytes: 0,
                network_rx_bytes: 0,
                network_tx_bytes: 0,
                storage_read_bytes: 0,
                storage_write_bytes: 0,
            }),
            evidence: Some(RuntimeEvidence {
                provider_build: self.config.build.clone(),
                spec_digest,
                semantics_profile_digest: spec.semantics_profile_digest.clone(),
                claims: BTreeMap::from([
                    (
                        "provider.class".to_string(),
                        "development-process".to_string(),
                    ),
                    ("a3s.runtime.contract".to_string(), "0.2.0".to_string()),
                ]),
            }),
            provider_attestation: None,
            failure: outcome.failure,
        })
    }
}

#[async_trait]
impl RuntimeDriver for ProcessRuntimeDriver {
    fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    async fn capabilities(&self) -> RuntimeResult<RuntimeCapabilities> {
        Ok(RuntimeCapabilities {
            schema: RuntimeCapabilities::SCHEMA.to_string(),
            provider_id: self.provider_id.clone(),
            provider_build: self.config.build.clone(),
            unit_classes: vec![RuntimeUnitClass::Task],
            artifact_media_types: vec!["application/vnd.a3s.workflow.node-runner.v1".to_string()],
            isolation_levels: vec![IsolationLevel::Process],
            network_modes: vec![NetworkMode::None, NetworkMode::Outbound],
            mount_kinds: vec![MountKind::Artifact],
            health_check_kinds: Vec::<HealthCheckKind>::new(),
            resource_controls: vec![
                ResourceControl::Cpu,
                ResourceControl::Memory,
                ResourceControl::Pids,
                ResourceControl::EphemeralStorage,
                ResourceControl::ExecutionTimeout,
            ],
            features: vec![
                RuntimeFeature::DurableIdentity,
                RuntimeFeature::Stop,
                RuntimeFeature::Remove,
                RuntimeFeature::Logs,
                RuntimeFeature::SecretReferences,
                RuntimeFeature::OutputArtifacts,
                RuntimeFeature::Usage,
            ],
        })
    }

    async fn apply(
        &self,
        spec: &a3s_runtime::contract::RuntimeUnitSpec,
        _current: &RuntimeObservation,
    ) -> RuntimeResult<RuntimeObservation> {
        if spec.class != RuntimeUnitClass::Task || spec.isolation != IsolationLevel::Process {
            return Err(RuntimeError::UnsupportedCapabilities(vec![
                "development provider only runs process-isolated tasks".to_string(),
            ]));
        }
        let runner = self.resolve_runner(&spec.artifact).await?;
        let sandbox = TempDir::new().map_err(|error| RuntimeError::Transport(error.to_string()))?;
        let root = sandbox.path();
        let mapped = self.materialize_mounts(spec, root).await?;
        let mut environment = spec.process.environment.clone();
        for value in environment.values_mut() {
            if let Some(path) = mapped.get(value) {
                *value = path.to_string_lossy().into_owned();
            } else if spec.outputs.iter().any(|output| output.path == *value) {
                *value = sandbox_path(root, value)?.to_string_lossy().into_owned();
            }
        }
        self.materialize_secrets(spec, root, &mut environment)
            .await?;

        let mut command = if spec.process.command.is_empty() {
            Command::new(&runner)
        } else {
            let mut command = Command::new(&spec.process.command[0]);
            command.args(&spec.process.command[1..]);
            command
        };
        command
            .args(&spec.process.args)
            .envs(&environment)
            .current_dir(root)
            .kill_on_drop(true);
        let resource_id = format!("process-{}", Uuid::new_v4());
        let started_at_ms = now_ms();
        let started = Instant::now();
        let timeout = Duration::from_millis(
            spec.resources
                .execution_timeout_ms
                .ok_or_else(|| RuntimeError::Protocol("task timeout missing".to_string()))?,
        );
        let output = match tokio::time::timeout(timeout, command.output()).await {
            Ok(result) => result.map_err(|error| RuntimeError::Transport(error.to_string()))?,
            Err(_) => {
                return self.terminal_observation(
                    spec,
                    resource_id,
                    started_at_ms,
                    started.elapsed().as_millis() as u64,
                    TerminalOutcome {
                        state: RuntimeUnitState::Failed,
                        outputs: Vec::new(),
                        failure: Some(RuntimeFailure {
                            code: "execution-timeout".to_string(),
                            message: "node execution exceeded its Runtime timeout".to_string(),
                            retryable: true,
                        }),
                    },
                );
            }
        };
        self.record_logs(spec, &output.stdout, &output.stderr).await;
        if !output.status.success() {
            return self.terminal_observation(
                spec,
                resource_id,
                started_at_ms,
                started.elapsed().as_millis() as u64,
                TerminalOutcome {
                    state: RuntimeUnitState::Failed,
                    outputs: Vec::new(),
                    failure: Some(RuntimeFailure {
                        code: "process-failed".to_string(),
                        message: bounded_message(&output.stderr),
                        retryable: false,
                    }),
                },
            );
        }
        let outputs = self.persist_outputs(spec, root).await?;
        self.terminal_observation(
            spec,
            resource_id,
            started_at_ms,
            started.elapsed().as_millis() as u64,
            TerminalOutcome {
                state: RuntimeUnitState::Succeeded,
                outputs,
                failure: None,
            },
        )
    }

    async fn inspect(&self, unit: &RuntimeUnitRecord) -> RuntimeResult<RuntimeInspection> {
        Ok(RuntimeInspection::Found {
            schema: RuntimeInspection::SCHEMA.to_string(),
            observation: Box::new(unit.observation.clone()),
        })
    }

    async fn stop(
        &self,
        unit: &RuntimeUnitRecord,
        _request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeObservation> {
        Ok(unit.observation.clone())
    }

    async fn remove(
        &self,
        unit: &RuntimeUnitRecord,
        request: &RuntimeActionRequest,
    ) -> RuntimeResult<RuntimeRemoval> {
        Ok(RuntimeRemoval {
            schema: RuntimeRemoval::SCHEMA.to_string(),
            request_id: request.request_id.clone(),
            unit_id: unit.spec.unit_id.clone(),
            generation: unit.spec.generation,
            removed_at_ms: now_ms(),
            already_absent: false,
        })
    }

    async fn logs(
        &self,
        unit: &RuntimeUnitRecord,
        query: &RuntimeLogQuery,
    ) -> RuntimeResult<Vec<RuntimeLogChunk>> {
        let logs = self.logs.read().await;
        let chunks = logs
            .get(&(unit.spec.unit_id.clone(), unit.spec.generation))
            .cloned()
            .unwrap_or_default();
        Ok(chunks
            .into_iter()
            .filter(|chunk| query.stream.is_none_or(|stream| stream == chunk.stream))
            .take(query.limit as usize)
            .collect())
    }

    async fn exec(
        &self,
        _unit: &RuntimeUnitRecord,
        _request: &RuntimeExecRequest,
    ) -> RuntimeResult<RuntimeExecResult> {
        Err(RuntimeError::UnsupportedCapabilities(vec![
            "feature:Exec".to_string()
        ]))
    }
}

fn sandbox_path(root: &Path, absolute: &str) -> RuntimeResult<PathBuf> {
    let relative = absolute.strip_prefix('/').ok_or_else(|| {
        RuntimeError::InvalidRequest(format!("path {absolute:?} is not absolute"))
    })?;
    if relative.split('/').any(|part| part == "..") {
        return Err(RuntimeError::InvalidRequest(
            "sandbox path contains traversal".to_string(),
        ));
    }
    Ok(root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn verify_digest(bytes: &[u8], expected: &str) -> RuntimeResult<()> {
    if digest(bytes) == expected {
        Ok(())
    } else {
        Err(RuntimeError::Protocol(
            "artifact digest verification failed".to_string(),
        ))
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn bounded_message(bytes: &[u8]) -> String {
    let message = String::from_utf8_lossy(bytes);
    let message = message.trim();
    if message.is_empty() {
        "node process exited unsuccessfully".to_string()
    } else {
        message.chars().take(16_000).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_paths_never_escape_the_task_root() {
        let root = Path::new("sandbox");
        assert_eq!(
            sandbox_path(root, "/a3s/input.json").expect("safe path"),
            root.join("a3s").join("input.json")
        );
        assert!(sandbox_path(root, "/../secret").is_err());
    }
}
