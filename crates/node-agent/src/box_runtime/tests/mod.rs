use super::*;
use crate::{NodeControlClientError, NodeSecretTransport, SecretMaterial};
use a3s_box_runtime::{ImageStore, DEFAULT_IMAGE_CACHE_SIZE};
use a3s_cloud_contracts::CloudSecretReference;
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeExecRequest, RuntimeInspection, RuntimeLogQuery,
    RuntimeNetworkSpec, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec, RuntimeUnitState,
    SecretReference, SecretTarget,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tokio::time::Instant;
use uuid::Uuid;

#[path = "private_registry.rs"]
mod private_registry;

struct GateSecretTransport {
    materials: HashMap<String, Vec<u8>>,
    calls: Mutex<HashMap<String, Arc<AtomicUsize>>>,
}

impl GateSecretTransport {
    fn new(materials: impl IntoIterator<Item = (CloudSecretReference, Vec<u8>)>) -> Self {
        let mut values = HashMap::new();
        let mut calls = HashMap::new();
        for (reference, material) in materials {
            let reference = reference.to_string();
            values.insert(reference.clone(), material);
            calls.insert(reference, Arc::new(AtomicUsize::new(0)));
        }
        Self {
            materials: values,
            calls: Mutex::new(calls),
        }
    }

    fn calls(&self, reference: CloudSecretReference) -> usize {
        self.calls
            .lock()
            .expect("Secret call counters")
            .get(&reference.to_string())
            .expect("registered Secret call counter")
            .load(Ordering::SeqCst)
    }
}

#[async_trait]
impl NodeSecretTransport for GateSecretTransport {
    async fn resolve_secret(
        &self,
        reference: CloudSecretReference,
    ) -> Result<SecretMaterial, NodeControlClientError> {
        let reference = reference.to_string();
        self.calls
            .lock()
            .map_err(|_| NodeControlClientError::Transport("Secret counter poisoned".into()))?
            .get(&reference)
            .ok_or_else(|| {
                NodeControlClientError::Invalid("Secret reference is not registered".into())
            })?
            .fetch_add(1, Ordering::SeqCst);
        let value = self.materials.get(&reference).cloned().ok_or_else(|| {
            NodeControlClientError::Invalid("Secret material is not registered".into())
        })?;
        SecretMaterial::new(value).map_err(NodeControlClientError::Invalid)
    }
}

fn config(
    isolation: BoxRuntimeIsolation,
) -> (tempfile::TempDir, tempfile::TempDir, BoxRuntimeConfig) {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().expect("temporary Box home");
    let secret_mount = tempfile::Builder::new()
        .prefix("a3s-cloud-box-secrets-")
        .tempdir_in("/dev/shm")
        .expect("temporary Box Secret tmpfs mount");
    let secret_root = secret_mount.path().join("runtime-secrets");
    std::fs::create_dir(&secret_root).expect("create private Box Secret root");
    std::fs::set_permissions(&secret_root, std::fs::Permissions::from_mode(0o700))
        .expect("make Box Secret root provider-private");
    let config = BoxRuntimeConfig {
        home_dir: home.path().to_path_buf(),
        secret_root,
        isolation,
        control_timeout_ms: 60_000,
        task_poll_interval_ms: 50,
    };
    (home, secret_mount, config)
}

#[tokio::test]
async fn selects_the_exact_configured_box_isolation_and_secret_capability_without_fallback() {
    for (configured, expected) in [
        (BoxRuntimeIsolation::Microvm, ExecutionIsolation::Microvm),
        (BoxRuntimeIsolation::Sandbox, ExecutionIsolation::Sandbox),
    ] {
        let (_home, _secret_root, config) = config(configured);
        let materializer = Arc::new(CloudBoxSecretMaterializer::new());
        let driver = build_box_runtime_driver(&config, materializer).expect("Box Runtime driver");

        assert_eq!(driver.execution_isolation(), expected);
        assert!(driver
            .capabilities()
            .await
            .expect("Box Runtime capabilities")
            .features
            .contains(&a3s_runtime::contract::RuntimeFeature::SecretReferences));
    }
}

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_BOX=1 on the dedicated real Box provider runner"]
async fn real_box_materializes_cloud_secrets_redacts_logs_and_cleans_tmpfs(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "dedicated Box gate did not enable real-provider tests",
        )
        .into());
    }
    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let secret_root = home.join("runtime-secrets").canonicalize()?;
    let runtime_state = tempfile::tempdir()?;
    let workload_revision_id = Uuid::now_v7();
    let environment_reference = CloudSecretReference::new(workload_revision_id, Uuid::now_v7(), 3)
        .map_err(io::Error::other)?;
    let file_reference = CloudSecretReference::new(workload_revision_id, Uuid::now_v7(), 5)
        .map_err(io::Error::other)?;
    let registry_reference = CloudSecretReference::new(workload_revision_id, Uuid::now_v7(), 7)
        .map_err(io::Error::other)?;
    let environment_value = format!("cloud-env-secret-{}", Uuid::now_v7().simple());
    let file_value = format!("cloud-file-secret-{}", Uuid::now_v7().simple());
    let registry_password = format!("cloud-registry-secret-{}", Uuid::now_v7().simple());
    let registry_material = serde_json::to_vec(&serde_json::json!({
        "schema": a3s_cloud_contracts::RegistryCredentialMaterial::SCHEMA,
        "username": "cloud-registry-user",
        "password": registry_password.clone(),
    }))?;
    let transport = Arc::new(GateSecretTransport::new([
        (environment_reference, environment_value.as_bytes().to_vec()),
        (file_reference, file_value.as_bytes().to_vec()),
        (registry_reference, registry_material),
    ]));
    let config = BoxRuntimeConfig {
        home_dir: home.clone(),
        secret_root: secret_root.clone(),
        isolation: BoxRuntimeIsolation::Sandbox,
        control_timeout_ms: 120_000,
        task_poll_interval_ms: 25,
    };
    let provider = build_box_runtime_provider(&config, runtime_state.path())?;
    let binding: Arc<dyn NodeSecretTransport> = transport.clone();
    provider.bind_secret_transport(binding).await?;
    let runtime = provider.into_client();
    let spec = secret_service_spec(
        environment_reference,
        file_reference,
        registry_reference,
        &environment_value,
        &file_value,
    )?;
    let request = RuntimeApplyRequest {
        schema: RuntimeApplyRequest::SCHEMA.into(),
        request_id: format!("cloud-box-secret-apply-{}", Uuid::now_v7()),
        deadline_at_ms: None,
        spec: spec.clone(),
    };

    let running = runtime.apply(&request).await?;
    if running.state != a3s_runtime::contract::RuntimeUnitState::Running {
        return Err(io::Error::other("Secret fixture Service did not reach Running").into());
    }
    if transport.calls(environment_reference) != 1
        || transport.calls(file_reference) != 1
        || transport.calls(registry_reference) != 0
    {
        return Err(io::Error::other(
            "Box did not resolve only workload Secrets on the cached image path",
        )
        .into());
    }

    drop(runtime);
    let recovered_provider = build_box_runtime_provider(&config, runtime_state.path())?;
    let binding: Arc<dyn NodeSecretTransport> = transport.clone();
    recovered_provider.bind_secret_transport(binding).await?;
    let recovered = recovered_provider.into_client();
    let calls_before_inspection = (
        transport.calls(environment_reference),
        transport.calls(file_reference),
        transport.calls(registry_reference),
    );
    if !matches!(
        recovered.inspect(&spec.unit_id).await?,
        a3s_runtime::contract::RuntimeInspection::Found { .. }
    ) || calls_before_inspection
        != (
            transport.calls(environment_reference),
            transport.calls(file_reference),
            transport.calls(registry_reference),
        )
    {
        return Err(
            io::Error::other("driver reconstruction re-resolved a running Secret binding").into(),
        );
    }

    let calls_before_logs = (
        transport.calls(environment_reference),
        transport.calls(file_reference),
    );
    let chunks = wait_for_redacted_logs(recovered.as_ref(), &spec).await?;
    let projected = chunks
        .iter()
        .map(|chunk| chunk.data.as_str())
        .collect::<String>();
    if projected.contains(&environment_value)
        || projected.contains(&file_value)
        || projected.contains(&registry_password)
        || !projected.contains("[REDACTED]")
        || transport.calls(environment_reference) <= calls_before_logs.0
        || transport.calls(file_reference) <= calls_before_logs.1
        || transport.calls(registry_reference) != 0
    {
        return Err(io::Error::other(
            "Box log projection leaked a Secret, skipped reauthorization, or resolved a registry credential",
        )
        .into());
    }

    let calls_before_restart = (
        transport.calls(environment_reference),
        transport.calls(file_reference),
    );
    let restart_trigger = recovered
        .exec(&RuntimeExecRequest {
            schema: RuntimeExecRequest::SCHEMA.into(),
            request_id: format!("cloud-box-secret-restart-{}", Uuid::now_v7()),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            command: vec![
                "/bin/sh".into(),
                "-c".into(),
                "touch /tmp/a3s-cloud-secret-restart".into(),
            ],
            timeout_ms: 10_000,
            deadline_at_ms: None,
        })
        .await?;
    if restart_trigger.exit_code != 0 {
        return Err(io::Error::other("Secret restart trigger failed").into());
    }
    wait_for_secret_restart(
        recovered.as_ref(),
        &spec,
        transport.as_ref(),
        environment_reference,
        file_reference,
        calls_before_restart,
    )
    .await?;
    if transport.calls(registry_reference) != 0 {
        return Err(
            io::Error::other("cached restart resolved an unnecessary registry credential").into(),
        );
    }
    let restarted_logs = wait_for_redacted_logs(recovered.as_ref(), &spec).await?;
    if restarted_logs.iter().any(|chunk| {
        chunk.data.contains(&environment_value)
            || chunk.data.contains(&file_value)
            || chunk.data.contains(&registry_password)
    }) {
        return Err(io::Error::other("restarted Box logs leaked Secret plaintext").into());
    }

    recovered
        .remove(&RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("cloud-box-secret-remove-{}", Uuid::now_v7()),
            unit_id: spec.unit_id.clone(),
            generation: spec.generation,
            deadline_at_ms: None,
        })
        .await?;
    if secret_root.read_dir()?.next().is_some() {
        return Err(io::Error::other("Box cleanup left materialized Secret files").into());
    }
    exercise_private_registry(
        &home,
        &secret_root,
        transport,
        registry_reference,
        "cloud-registry-user",
        &registry_password,
    )
    .await?;
    assert_no_plaintext(
        &[home.as_path(), runtime_state.path()],
        &[&environment_value, &file_value, &registry_password],
    )?;
    println!(
        "A3S_CLOUD_BOX_SECRET_MATERIALIZATION_CERTIFIED unit={} generation={}",
        spec.unit_id, spec.generation
    );
    Ok(())
}

async fn exercise_private_registry(
    source_home: &Path,
    shared_secret_root: &Path,
    transport: Arc<GateSecretTransport>,
    registry_reference: CloudSecretReference,
    registry_username: &str,
    registry_password: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::var("A3S_REGISTRY_PROTOCOL").as_deref() != Ok("http") {
        return Err(io::Error::other(
            "private-registry gate requires loopback-only A3S_REGISTRY_PROTOCOL=http",
        )
        .into());
    }
    let source_reference = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let source_store = ImageStore::new(&source_home.join("images"), DEFAULT_IMAGE_CACHE_SIZE)?;
    let source_image = source_store.get(&source_reference).await.ok_or_else(|| {
        io::Error::other("public conformance image was not cached before the private pull")
    })?;
    let registry = private_registry::PrivateRegistry::start(
        &source_image.path,
        registry_username,
        registry_password,
    )
    .await?;
    let anonymous = reqwest::Client::new()
        .get(registry.protected_manifest_url())
        .send()
        .await?;
    if anonymous.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Err(
            io::Error::other("private registry accepted an anonymous protected request").into(),
        );
    }

    let parent = source_home
        .parent()
        .ok_or_else(|| io::Error::other("Box conformance home has no parent"))?;
    let private_home = tempfile::Builder::new()
        .prefix("a3s-cloud-private-registry-")
        .tempdir_in(parent)?;
    let private_secret_root = tempfile::Builder::new()
        .prefix("private-registry-")
        .tempdir_in(shared_secret_root)?;
    let config = BoxRuntimeConfig {
        home_dir: private_home.path().to_path_buf(),
        secret_root: private_secret_root.path().to_path_buf(),
        isolation: BoxRuntimeIsolation::Sandbox,
        control_timeout_ms: 120_000,
        task_poll_interval_ms: 25,
    };
    let provider = build_box_runtime_provider(&config, private_home.path().join("runtime-state"))?;
    let binding: Arc<dyn NodeSecretTransport> = transport.clone();
    provider.bind_secret_transport(binding).await?;
    let runtime = provider.into_client();
    let calls_before_pull = transport.calls(registry_reference);
    let first = private_registry_service_spec(&registry, registry_reference)?;
    let running = runtime
        .apply(&RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("cloud-private-registry-apply-{}", Uuid::now_v7()),
            deadline_at_ms: None,
            spec: first.clone(),
        })
        .await?;
    if running.state != RuntimeUnitState::Running
        || transport.calls(registry_reference) != calls_before_pull + 1
    {
        return Err(io::Error::other(
            "uncached private pull did not resolve one Cloud registry credential",
        )
        .into());
    }
    registry.assert_authenticated_pull()?;
    if private_home.path().join("auth/credentials.json").exists() {
        return Err(io::Error::other(
            "transient registry credential entered the persistent Box auth store",
        )
        .into());
    }
    runtime
        .remove(&RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("cloud-private-registry-remove-{}", Uuid::now_v7()),
            unit_id: first.unit_id.clone(),
            generation: first.generation,
            deadline_at_ms: None,
        })
        .await?;

    let requests_before_cache_hit = registry.request_count()?;
    let cached = private_registry_service_spec(&registry, registry_reference)?;
    let running = runtime
        .apply(&RuntimeApplyRequest {
            schema: RuntimeApplyRequest::SCHEMA.into(),
            request_id: format!("cloud-private-registry-cache-{}", Uuid::now_v7()),
            deadline_at_ms: None,
            spec: cached.clone(),
        })
        .await?;
    if running.state != RuntimeUnitState::Running
        || transport.calls(registry_reference) != calls_before_pull + 1
        || registry.request_count()? != requests_before_cache_hit
    {
        return Err(io::Error::other(
            "cached private image reloaded its credential or contacted the registry",
        )
        .into());
    }
    runtime
        .remove(&RuntimeActionRequest {
            schema: RuntimeActionRequest::SCHEMA.into(),
            request_id: format!("cloud-private-registry-cache-remove-{}", Uuid::now_v7()),
            unit_id: cached.unit_id.clone(),
            generation: cached.generation,
            deadline_at_ms: None,
        })
        .await?;
    if private_secret_root.path().read_dir()?.next().is_some() {
        return Err(
            io::Error::other("private-registry cleanup left transient Secret material").into(),
        );
    }
    assert_no_plaintext(
        &[private_home.path(), private_secret_root.path()],
        &[registry_username, registry_password],
    )?;
    Ok(())
}

fn private_registry_service_spec(
    registry: &private_registry::PrivateRegistry,
    registry_reference: CloudSecretReference,
) -> Result<RuntimeUnitSpec, Box<dyn std::error::Error + Send + Sync>> {
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("cloud-private-registry-{}", Uuid::now_v7().simple()),
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!("oci://{}", registry.image_reference()),
            digest: registry.index_digest.clone(),
            media_type: private_registry::OCI_IMAGE_INDEX.into(),
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-c".into()],
            args: vec![
                "printf 'cloud-private-registry-ready\\n'; while :; do sleep 60; done".into(),
            ],
            working_directory: Some("/".into()),
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: vec![SecretReference {
            name: "registry".into(),
            reference: registry_reference.to_string(),
            target: SecretTarget::RegistryCredential,
        }],
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    };
    spec.validate().map_err(io::Error::other)?;
    Ok(spec)
}

fn secret_service_spec(
    environment_reference: CloudSecretReference,
    file_reference: CloudSecretReference,
    registry_reference: CloudSecretReference,
    environment_value: &str,
    file_value: &str,
) -> Result<RuntimeUnitSpec, Box<dyn std::error::Error + Send + Sync>> {
    let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let (repository, digest) = image
        .rsplit_once('@')
        .ok_or_else(|| io::Error::other("Box conformance image is not digest-pinned"))?;
    let environment_digest = format!("{:x}", Sha256::digest(environment_value.as_bytes()));
    let file_digest = format!("{:x}", Sha256::digest(file_value.as_bytes()));
    let script = format!(
        "rm -f /tmp/a3s-cloud-secret-restart; test \"$(printf %s \"$CLOUD_ENV_SECRET\" | sha256sum | cut -d' ' -f1)\" = \"{environment_digest}\" || exit 71; test \"$(sha256sum /run/secrets/cloud-file | cut -d' ' -f1)\" = \"{file_digest}\" || exit 72; test \"$(stat -c %a /run/secrets/cloud-file)\" = 400 || exit 73; printf 'stdout:%s:%s\\n' \"$CLOUD_ENV_SECRET\" \"$(cat /run/secrets/cloud-file)\"; printf 'stderr:%s:%s\\n' \"$CLOUD_ENV_SECRET\" \"$(cat /run/secrets/cloud-file)\" >&2; while test ! -e /tmp/a3s-cloud-secret-restart; do sleep 1; done; rm -f /tmp/a3s-cloud-secret-restart; exit 91"
    );
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("cloud-box-secret-{}", Uuid::now_v7().simple()),
        generation: 1,
        class: RuntimeUnitClass::Service,
        artifact: ArtifactRef {
            uri: format!("oci://{repository}@{digest}"),
            digest: digest.into(),
            media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-c".into()],
            args: vec![script],
            working_directory: Some("/".into()),
            environment: BTreeMap::new(),
        },
        mounts: Vec::new(),
        secrets: vec![
            SecretReference {
                name: "environment".into(),
                reference: environment_reference.to_string(),
                target: SecretTarget::Environment {
                    variable: "CLOUD_ENV_SECRET".into(),
                },
            },
            SecretReference {
                name: "file".into(),
                reference: file_reference.to_string(),
                target: SecretTarget::File {
                    path: "/run/secrets/cloud-file".into(),
                    mode: 0o400,
                },
            },
            SecretReference {
                name: "registry".into(),
                reference: registry_reference.to_string(),
                target: SecretTarget::RegistryCredential,
            },
        ],
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: None,
        },
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: RestartPolicy::Always,
        outputs: Vec::new(),
        semantics_profile_digest: None,
    };
    spec.validate().map_err(io::Error::other)?;
    Ok(spec)
}

async fn wait_for_redacted_logs(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
) -> Result<Vec<a3s_runtime::contract::RuntimeLogChunk>, a3s_runtime::RuntimeError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let chunks = runtime
            .logs(&RuntimeLogQuery {
                schema: RuntimeLogQuery::SCHEMA.into(),
                unit_id: spec.unit_id.clone(),
                generation: spec.generation,
                cursor: None,
                limit: 100,
                stream: None,
            })
            .await?;
        if chunks.iter().any(|chunk| chunk.data.contains("[REDACTED]")) {
            return Ok(chunks);
        }
        if Instant::now() >= deadline {
            return Err(a3s_runtime::RuntimeError::ProviderUnavailable(
                "Box Secret fixture logs did not become observable".into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_secret_restart(
    runtime: &dyn RuntimeClient,
    spec: &RuntimeUnitSpec,
    transport: &GateSecretTransport,
    environment_reference: CloudSecretReference,
    file_reference: CloudSecretReference,
    calls_before_restart: (usize, usize),
) -> Result<(), a3s_runtime::RuntimeError> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let RuntimeInspection::Found { observation, .. } = runtime.inspect(&spec.unit_id).await?
        {
            if observation.state == RuntimeUnitState::Running
                && transport.calls(environment_reference) > calls_before_restart.0
                && transport.calls(file_reference) > calls_before_restart.1
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(a3s_runtime::RuntimeError::ProviderUnavailable(
                "Box Secret fixture did not restart with fresh materialization".into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn assert_no_plaintext(roots: &[&Path], values: &[&str]) -> io::Result<()> {
    for root in roots {
        scan_plaintext(root, values)?;
    }
    Ok(())
}

fn scan_plaintext(path: &Path, values: &[&str]) -> io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            scan_plaintext(&entry?.path(), values)?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        let bytes = std::fs::read(path)?;
        if values.iter().any(|value| {
            bytes
                .windows(value.len())
                .any(|window| window == value.as_bytes())
        }) {
            return Err(io::Error::other(format!(
                "Secret plaintext remained in {}",
                path.display()
            )));
        }
    }
    Ok(())
}
