use super::DockerRuntimeDriver;
use crate::{NodeControlClientError, SecretMaterial};
use a3s_cloud_contracts::CloudSecretReference;
use a3s_runtime::contract::{RuntimeUnitSpec, SecretTarget};
use a3s_runtime::{RuntimeError, RuntimeResult};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub(super) struct MaterializedSecrets {
    environment: Vec<(String, SecretMaterial)>,
    files: Vec<MaterializedSecretFile>,
}

struct MaterializedSecretFile {
    source: PathBuf,
    target: String,
}

impl MaterializedSecrets {
    pub(super) fn environment(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.environment
            .iter()
            .map(|(variable, material)| (variable.as_str(), material.as_bytes()))
    }

    pub(super) fn files(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.files
            .iter()
            .map(|file| (file.source.as_path(), file.target.as_str()))
    }
}

impl DockerRuntimeDriver {
    pub(super) async fn resolve_log_redaction_materials(
        &self,
        spec: &RuntimeUnitSpec,
    ) -> RuntimeResult<Vec<SecretMaterial>> {
        if spec.secrets.is_empty() {
            return Ok(Vec::new());
        }
        let transport = self.secret_transport.read().await.clone().ok_or_else(|| {
            RuntimeError::ProviderUnavailable(
                "Docker Secret material transport is not bound".into(),
            )
        })?;
        let mut materials = Vec::with_capacity(spec.secrets.len());
        for secret in &spec.secrets {
            let reference = CloudSecretReference::parse(&secret.reference).map_err(|_| {
                RuntimeError::InvalidRequest(
                    "Docker log redaction Secret reference is invalid".into(),
                )
            })?;
            materials.push(
                transport
                    .resolve_secret(reference)
                    .await
                    .map_err(secret_transport_error)?,
            );
        }
        Ok(materials)
    }

    pub(super) async fn materialize_secrets(
        &self,
        spec: &RuntimeUnitSpec,
        spec_digest: &str,
    ) -> RuntimeResult<MaterializedSecrets> {
        let transport = self.secret_transport.read().await.clone().ok_or_else(|| {
            RuntimeError::ProviderUnavailable(
                "Docker Secret material transport is not bound".into(),
            )
        })?;
        let has_files = spec
            .secrets
            .iter()
            .any(|secret| matches!(secret.target, SecretTarget::File { .. }));
        let directory = if has_files {
            Some(self.prepare_secret_directory(spec_digest).await?)
        } else {
            None
        };
        let mut environment = Vec::new();
        let mut files = Vec::new();
        for (index, secret) in spec.secrets.iter().enumerate() {
            let reference = match CloudSecretReference::parse(&secret.reference) {
                Ok(reference) => reference,
                Err(error) => {
                    return self
                        .materialization_error(directory.as_deref(), &error)
                        .await
                }
            };
            let material = match transport.resolve_secret(reference).await {
                Ok(material) => material,
                Err(error) => {
                    if let Some(directory) = directory.as_deref() {
                        tokio::fs::remove_dir_all(directory)
                            .await
                            .map_err(|cleanup| {
                                RuntimeError::ProviderUnavailable(format!(
                                "could not clean failed Docker Secret materialization: {cleanup}"
                            ))
                            })?;
                    }
                    return Err(secret_transport_error(error));
                }
            };
            match &secret.target {
                SecretTarget::Environment { variable } => {
                    if std::str::from_utf8(material.as_bytes()).is_err()
                        || material.as_bytes().contains(&0)
                        || spec.process.environment.contains_key(variable)
                        || environment.iter().any(|(existing, _)| existing == variable)
                    {
                        return self
                            .materialization_error(
                                directory.as_deref(),
                                "Docker Secret environment target is invalid",
                            )
                            .await;
                    }
                    environment.push((variable.clone(), material));
                }
                SecretTarget::File { path, mode } => {
                    if spec.mounts.iter().any(|mount| mount.target == *path) {
                        return self
                            .materialization_error(
                                directory.as_deref(),
                                "Docker Secret file target overlaps a Runtime mount",
                            )
                            .await;
                    }
                    let directory = directory.as_deref().ok_or_else(|| {
                        RuntimeError::Protocol(
                            "Docker Secret file directory was not prepared".into(),
                        )
                    })?;
                    match write_secret_file(directory, index, *mode, material.as_bytes()).await {
                        Ok(source) => files.push(MaterializedSecretFile {
                            source,
                            target: path.clone(),
                        }),
                        Err(error) => {
                            self.cleanup_secret_directory(spec_digest).await?;
                            return Err(error);
                        }
                    }
                }
            }
        }
        Ok(MaterializedSecrets { environment, files })
    }

    pub(super) async fn cleanup_secret_directory(&self, spec_digest: &str) -> RuntimeResult<()> {
        let directory = self.secret_directory(spec_digest)?;
        match tokio::fs::remove_dir_all(&directory).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(RuntimeError::ProviderUnavailable(format!(
                "could not remove Docker Secret memory directory: {error}"
            ))),
        }
    }

    async fn materialization_error<T>(
        &self,
        directory: Option<&Path>,
        message: &str,
    ) -> RuntimeResult<T> {
        if let Some(directory) = directory {
            match tokio::fs::remove_dir_all(directory).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(RuntimeError::ProviderUnavailable(format!(
                        "could not remove invalid Docker Secret material: {error}"
                    )))
                }
            }
        }
        Err(RuntimeError::InvalidRequest(message.into()))
    }

    async fn prepare_secret_directory(&self, spec_digest: &str) -> RuntimeResult<PathBuf> {
        tokio::fs::create_dir_all(&self.secret_memory_dir)
            .await
            .map_err(|error| {
                RuntimeError::ProviderUnavailable(format!(
                    "could not create Docker Secret memory root: {error}"
                ))
            })?;
        secure_directory(&self.secret_memory_dir).await?;
        ensure_memory_backed(&self.secret_memory_dir).await?;
        let directory = self.secret_directory(spec_digest)?;
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|error| {
                RuntimeError::ProviderUnavailable(format!(
                    "could not create Docker Secret generation directory: {error}"
                ))
            })?;
        secure_directory(&directory).await?;
        Ok(directory)
    }

    fn secret_directory(&self, spec_digest: &str) -> RuntimeResult<PathBuf> {
        let digest = spec_digest
            .strip_prefix("sha256:")
            .filter(|value| {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| {
                RuntimeError::Protocol("Docker Secret specification digest is invalid".into())
            })?;
        Ok(self.secret_memory_dir.join(digest))
    }
}

async fn write_secret_file(
    directory: &Path,
    index: usize,
    mode: u32,
    material: &[u8],
) -> RuntimeResult<PathBuf> {
    if mode == 0 || mode > 0o777 {
        return Err(RuntimeError::InvalidRequest(
            "Docker Secret file mode is invalid".into(),
        ));
    }
    let target = directory.join(format!("secret-{index}"));
    let temporary = directory.join(format!(".secret-{index}-{}", Uuid::now_v7().simple()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not create Docker Secret memory file: {error}"
        ))
    })?;
    let write_result = async {
        file.write_all(material).await?;
        file.sync_all().await?;
        #[cfg(unix)]
        tokio::fs::set_permissions(
            &temporary,
            std::os::unix::fs::PermissionsExt::from_mode(mode),
        )
        .await?;
        tokio::fs::rename(&temporary, &target).await
    }
    .await;
    if let Err(error) = write_result {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(RuntimeError::ProviderUnavailable(format!(
            "could not commit Docker Secret memory file: {error}"
        )));
    }
    Ok(target)
}

async fn secure_directory(path: &Path) -> RuntimeResult<()> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not inspect Docker Secret memory directory: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(RuntimeError::InvalidRequest(
            "Docker Secret memory path must be a real directory".into(),
        ));
    }
    #[cfg(unix)]
    tokio::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .await
        .map_err(|error| {
            RuntimeError::ProviderUnavailable(format!(
                "could not secure Docker Secret memory directory: {error}"
            ))
        })?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn ensure_memory_backed(path: &Path) -> RuntimeResult<()> {
    let canonical = tokio::fs::canonicalize(path).await.map_err(|error| {
        RuntimeError::ProviderUnavailable(format!(
            "could not canonicalize Docker Secret memory directory: {error}"
        ))
    })?;
    let mounts = tokio::fs::read_to_string("/proc/self/mountinfo")
        .await
        .map_err(|error| {
            RuntimeError::ProviderUnavailable(format!(
                "could not inspect Linux mounts for Docker Secret memory: {error}"
            ))
        })?;
    let mut selected: Option<(usize, &str)> = None;
    for line in mounts.lines() {
        let Some((identity, filesystem)) = line.split_once(" - ") else {
            continue;
        };
        let Some(encoded_mount) = identity.split_whitespace().nth(4) else {
            continue;
        };
        let mount = PathBuf::from(decode_mount_path(encoded_mount));
        if canonical.starts_with(&mount) {
            let depth = mount.components().count();
            let kind = filesystem.split_whitespace().next().unwrap_or_default();
            if selected.is_none_or(|(selected_depth, _)| depth > selected_depth) {
                selected = Some((depth, kind));
            }
        }
    }
    if !matches!(selected, Some((_, "tmpfs"))) {
        return Err(RuntimeError::InvalidRequest(
            "Docker Secret files require a tmpfs-backed secret_memory_dir".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn decode_mount_path(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(not(target_os = "linux"))]
async fn ensure_memory_backed(_path: &Path) -> RuntimeResult<()> {
    Err(RuntimeError::UnsupportedCapabilities(vec![
        "feature:SecretFileInjectionRequiresLinuxTmpfs".into(),
    ]))
}

fn secret_transport_error(error: NodeControlClientError) -> RuntimeError {
    if error.retryable() {
        RuntimeError::ProviderUnavailable("Cloud Secret material is temporarily unavailable".into())
    } else {
        RuntimeError::InvalidRequest("Cloud Secret material request was rejected".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DockerConfig, NodeRuntimeBinding, NodeSecretTransport};
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy,
        RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, SecretReference,
    };
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    #[cfg(unix)]
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    #[cfg(unix)]
    use tempfile::TempDir;

    struct FixedSecretTransport {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl NodeSecretTransport for FixedSecretTransport {
        async fn resolve_secret(
            &self,
            _reference: CloudSecretReference,
        ) -> Result<SecretMaterial, NodeControlClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            SecretMaterial::new(b"materialized-at-docker".to_vec())
                .map_err(NodeControlClientError::Invalid)
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn environment_material_is_resolved_only_inside_the_docker_driver() {
        let (_socket_directory, _socket, driver) = test_driver("secret-unit-test");
        let transport = Arc::new(FixedSecretTransport {
            calls: AtomicUsize::new(0),
        });
        let binding: Arc<dyn NodeSecretTransport> = transport.clone();
        driver
            .bind_secret_transport(binding)
            .await
            .expect("Secret transport");
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 2).expect("reference");
        let spec = runtime_spec(reference.to_string());
        let digest = spec.digest().expect("spec digest");
        assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
        let materialized = driver
            .materialize_secrets(&spec, &digest)
            .await
            .expect("materialized Secrets");
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        let environment = materialized.environment().collect::<Vec<_>>();
        assert_eq!(
            environment,
            vec![("API_TOKEN", b"materialized-at-docker".as_slice())]
        );
        assert!(!serde_json::to_string(&spec)
            .expect("Runtime spec JSON")
            .contains("materialized-at-docker"));
    }

    struct NulSecretTransport;

    #[async_trait]
    impl NodeSecretTransport for NulSecretTransport {
        async fn resolve_secret(
            &self,
            _reference: CloudSecretReference,
        ) -> Result<SecretMaterial, NodeControlClientError> {
            SecretMaterial::new(b"invalid\0environment".to_vec())
                .map_err(NodeControlClientError::Invalid)
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn environment_material_rejects_nul_bytes() {
        let (_socket_directory, _socket, driver) = test_driver("secret-nul-test");
        let binding: Arc<dyn NodeSecretTransport> = Arc::new(NulSecretTransport);
        driver
            .bind_secret_transport(binding)
            .await
            .expect("Secret transport");
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 2).expect("reference");
        let spec = runtime_spec(reference.to_string());
        let digest = spec.digest().expect("spec digest");
        assert!(matches!(
            driver.materialize_secrets(&spec, &digest).await,
            Err(RuntimeError::InvalidRequest(_))
        ));
    }

    struct RejectedSecretTransport;

    #[async_trait]
    impl NodeSecretTransport for RejectedSecretTransport {
        async fn resolve_secret(
            &self,
            _reference: CloudSecretReference,
        ) -> Result<SecretMaterial, NodeControlClientError> {
            Err(NodeControlClientError::Rejected {
                status: 403,
                code: "forbidden".into(),
                message: "sensitive-control-plane-detail".into(),
                retryable: false,
            })
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn log_redaction_fails_closed_when_secret_authorization_is_rejected() {
        let (_socket_directory, _socket, driver) = test_driver("secret-log-redaction-test");
        let binding: Arc<dyn NodeSecretTransport> = Arc::new(RejectedSecretTransport);
        driver
            .bind_secret_transport(binding)
            .await
            .expect("Secret transport");
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 2).expect("reference");
        let error = driver
            .resolve_log_redaction_materials(&runtime_spec(reference.to_string()))
            .await
            .expect_err("rejected redaction material");
        assert!(matches!(error, RuntimeError::InvalidRequest(_)));
        assert!(!error.to_string().contains("sensitive-control-plane-detail"));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn file_material_is_written_only_to_tmpfs_with_the_requested_mode() {
        use std::os::unix::fs::PermissionsExt;

        let namespace = format!("secret-file-test-{}", Uuid::now_v7().simple());
        let (_socket_directory, _socket, driver) = test_driver(namespace);
        let binding: Arc<dyn NodeSecretTransport> = Arc::new(FixedSecretTransport {
            calls: AtomicUsize::new(0),
        });
        driver
            .bind_secret_transport(binding)
            .await
            .expect("Secret transport");
        let reference =
            CloudSecretReference::new(Uuid::now_v7(), Uuid::now_v7(), 1).expect("reference");
        let mut spec = runtime_spec(reference.to_string());
        spec.secrets[0].target = SecretTarget::File {
            path: "/run/secrets/api-token".into(),
            mode: 0o400,
        };
        let digest = spec.digest().expect("spec digest");
        let materialized = driver
            .materialize_secrets(&spec, &digest)
            .await
            .expect("materialized file");
        let (source, target) = materialized.files().next().expect("Secret file");
        assert_eq!(target, "/run/secrets/api-token");
        assert_eq!(
            tokio::fs::read(source).await.expect("Secret file contents"),
            b"materialized-at-docker"
        );
        assert_eq!(
            tokio::fs::metadata(source)
                .await
                .expect("Secret file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o400
        );
        drop(materialized);
        driver
            .cleanup_secret_directory(&digest)
            .await
            .expect("Secret generation cleanup");
        tokio::fs::remove_dir_all(&driver.secret_memory_dir)
            .await
            .expect("Secret test root cleanup");
    }

    #[cfg(unix)]
    fn test_driver(namespace: impl Into<String>) -> (TempDir, UnixListener, DockerRuntimeDriver) {
        let socket_directory = tempfile::tempdir().expect("Docker test socket directory");
        let socket_path = socket_directory.path().join("docker.sock");
        let socket = UnixListener::bind(&socket_path).expect("bind Docker test socket");
        let driver = DockerRuntimeDriver::connect(&DockerConfig {
            socket: format!("unix://{}", socket_path.display()),
            namespace: namespace.into(),
            operation_timeout_ms: 1_000,
            secret_memory_dir: "/dev/shm/a3s-cloud/test-secrets".into(),
        })
        .expect("Docker driver");
        (socket_directory, socket, driver)
    }

    fn runtime_spec(reference: String) -> RuntimeUnitSpec {
        let digest = format!("sha256:{}", "a".repeat(64));
        RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: "workload:test:revision:test".into(),
            generation: 1,
            class: RuntimeUnitClass::Service,
            artifact: ArtifactRef {
                uri: format!("oci://registry.example/test@{digest}"),
                digest,
                media_type: "application/vnd.oci.image.manifest.v1+json".into(),
            },
            process: RuntimeProcessSpec {
                command: Vec::new(),
                args: Vec::new(),
                working_directory: None,
                environment: BTreeMap::new(),
            },
            mounts: Vec::new(),
            secrets: vec![SecretReference {
                name: "api-token".into(),
                reference,
                target: SecretTarget::Environment {
                    variable: "API_TOKEN".into(),
                },
            }],
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
            restart: RestartPolicy::Always,
            outputs: Vec::new(),
            semantics_profile_digest: None,
        }
    }
}
