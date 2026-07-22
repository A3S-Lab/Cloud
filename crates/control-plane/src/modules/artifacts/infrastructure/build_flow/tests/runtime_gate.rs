use super::super::task_spec::{project_task_spec, BUILDKIT_SOCKET_ROOT};
use super::super::{BuildFlowConfig, BuildFlowConfigOptions};
use crate::modules::artifacts::domain::{
    BuildArtifact, BuildRunStatus, IBuildArtifactPublisher, IBuildOutputValidator,
    INodeArtifactStore, NodeArtifactDescriptor, OciPublicationRequest,
};
use crate::modules::artifacts::{
    LocalNodeArtifactStore, OciRegistryArtifactPublisher, OciRegistryArtifactPublisherOptions,
    RuntimeBuildOutputValidator,
};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    BuildRecipe, ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository,
    NewExternalSourceRevision,
};
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandOutcome,
    NodeCommandPayload, NodeCommandResult, RegistryCredentialMaterial,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_cloud_node_agent::{
    ArtifactConfig, CommandExecutor, DockerConfig, DockerRuntimeDriver, DownloadedNodeArtifact,
    FileCommandJournal, NodeArtifactManager, NodeArtifactTransport, NodeControlClientError,
    NodeRuntimeBinding,
};
use a3s_runtime::contract::{
    ArtifactRef, RuntimeActionRequest, RuntimeApplyRequest, RuntimeLogQuery, RuntimeLogStream,
    RuntimeObservation, RuntimeOutputArtifact, RuntimeUnitSpec, RuntimeUnitState,
};
use a3s_runtime::{
    FileRuntimeStateStore, ManagedRuntimeClient, RuntimeClient, RuntimeDriver, RuntimeStateStore,
};
use async_trait::async_trait;
use bollard::errors::Error as DockerError;
use bollard::{Docker, API_DEFAULT_VERSION};
use chrono::{Duration, Utc};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

mod busybox_rootfs;

use busybox_rootfs::validate_busybox_rootfs;

const GATE_ENV: &str = "A3S_CLOUD_TEST_RUNTIME_BUILDKIT";
const BUSYBOX_ROOTFS_ENV: &str = "A3S_CLOUD_TEST_BUSYBOX_ROOTFS";
const REGISTRY_URL_ENV: &str = "A3S_CLOUD_TEST_REGISTRY_URL";
const REGISTRY_USERNAME_ENV: &str = "A3S_CLOUD_TEST_REGISTRY_USERNAME";
const REGISTRY_PASSWORD_ENV: &str = "A3S_CLOUD_TEST_REGISTRY_PASSWORD";
const PUBLICATION_CREDENTIAL_ENV: &str = "A3S_CLOUD_TEST_RUNTIME_PUBLICATION_CREDENTIAL";
const DEFAULT_NAMESPACE: &str = "cloud-buildkit-gate";
const DEFAULT_VOLUME_ID: &str = "a3s-cloud-buildkit-v0-31-2";
const BUILDER_DIGEST: &str =
    "sha256:0eeb84626c0cd01aecae7848c5ed8f095aec279dd936d0cdb5a64110f42ca65b";
const NETWORK_CHECK_SCRIPT: &str = concat!(
    "/bin/busybox --list | /bin/busybox grep -qx wget || { ",
    "/bin/busybox echo A3S_NETWORK_CHECK_MISSING_WGET >&2; exit 41; }; ",
    "if /bin/busybox test -e /sys/class/net/eth0; then ",
    "/bin/busybox echo A3S_NETWORK_CHECK_ETH0_PRESENT >&2; exit 42; fi; ",
    "if /bin/busybox wget -T 1 -q -O /dev/null http://1.1.1.1/; then ",
    "/bin/busybox echo A3S_NETWORK_CHECK_EGRESS_REACHABLE >&2; exit 43; fi; ",
    "/bin/busybox echo A3S_NETWORK_CHECK_PASS",
);
const TASK_DIAGNOSTIC_BYTES: usize = 32 * 1024;

#[tokio::test]
#[ignore = "requires Docker, a rootless BuildKit socket volume, and an authenticated OCI registry"]
async fn real_runtime_task_builds_publishes_and_rejects_network_access(
) -> Result<(), Box<dyn Error>> {
    if std::env::var(GATE_ENV).as_deref() != Ok("1") {
        return Ok(());
    }

    let root = tempfile::tempdir()?;
    let namespace = std::env::var("A3S_CLOUD_TEST_RUNTIME_BUILDKIT_NAMESPACE")
        .unwrap_or_else(|_| DEFAULT_NAMESPACE.into());
    let volume_id = std::env::var("A3S_CLOUD_TEST_RUNTIME_BUILDKIT_VOLUME_ID")
        .unwrap_or_else(|_| DEFAULT_VOLUME_ID.into());
    let socket = std::env::var("A3S_CLOUD_TEST_DOCKER_SOCKET")
        .unwrap_or_else(|_| "unix:///var/run/docker.sock".into());
    let docker = docker(&socket)?;
    let expected_volume = runtime_volume_name(&namespace, &volume_id);
    docker
        .inspect_volume(&expected_volume)
        .await
        .map_err(|error| {
            std::io::Error::other(format!(
                "operator-provisioned BuildKit volume {expected_volume:?} is unavailable: {error}"
            ))
        })?;

    let artifacts: Arc<dyn INodeArtifactStore> = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("control-plane-artifacts"),
        1024 * 1024 * 1024,
    )?);
    let busybox_rootfs = tokio::fs::read(required_environment(BUSYBOX_ROOTFS_ENV)?).await?;
    validate_busybox_rootfs(&busybox_rootfs)?;
    let dockerfile = format!(
        "FROM scratch AS network-check\n\
         ADD busybox-rootfs.tar /\n\
         RUN [\"/bin/busybox\", \"sh\", \"-c\", {}]\n\
         FROM scratch\n\
         COPY --from=network-check /bin/busybox /network-check\n\
         COPY message.txt /message.txt\n",
        serde_json::to_string(NETWORK_CHECK_SCRIPT)?,
    );
    let source_archive = directory_archive(&[
        ("Dockerfile", dockerfile.as_bytes(), 0o644),
        ("busybox-rootfs.tar", busybox_rootfs.as_slice(), 0o444),
        (
            "message.txt",
            b"A3S Cloud Runtime BuildKit network-none gate\n",
            0o644,
        ),
    ])?;
    let input = admit_artifact(
        Arc::clone(&artifacts),
        root.path(),
        "source.tar",
        &source_archive,
    )
    .await?;
    let (mut build, revision) = prepared_build(input)?;
    let config = BuildFlowConfig::new(BuildFlowConfigOptions {
        builder: ArtifactRef {
            uri: format!("oci://docker.io/moby/buildkit@{BUILDER_DIGEST}"),
            digest: BUILDER_DIGEST.into(),
            media_type: "application/vnd.oci.image.index.v1+json".into(),
        },
        buildkit_socket_volume_id: volume_id,
        heartbeat_timeout_ms: 20_000,
        command_ttl_ms: 300_000,
        execution_timeout_ms: 240_000,
        observation_poll_ms: 100,
        convergence_timeout_ms: 300_000,
        cleanup_timeout_ms: 60_000,
        publication_timeout_ms: 60_000,
        cpu_millis: 2_000,
        memory_bytes: 1024 * 1024 * 1024,
        pids: 512,
        output_max_bytes: 512 * 1024 * 1024,
    })?;
    let spec = project_task_spec(&config, &build, &revision)?;
    let node_id = Uuid::now_v7();
    build.schedule(
        NodeId::from_uuid(node_id),
        spec.digest()?,
        build.updated_at + Duration::milliseconds(1),
    )?;
    let transport: Arc<dyn NodeArtifactTransport> =
        Arc::new(LocalArtifactTransport::new(Arc::clone(&artifacts)));
    let artifact_manager = Arc::new(NodeArtifactManager::new(
        root.path().join("node"),
        ArtifactConfig {
            max_blob_bytes: 512 * 1024 * 1024,
            max_entries: 100_000,
            max_file_bytes: 512 * 1024 * 1024,
            max_expanded_bytes: 1024 * 1024 * 1024,
        },
        node_id,
        transport,
    )?);
    let driver = Arc::new(DockerRuntimeDriver::connect(&DockerConfig {
        socket,
        namespace,
        operation_timeout_ms: 300_000,
        secret_memory_dir: root.path().join("secrets"),
    })?);
    driver.bind_node(node_id).await?;
    driver
        .bind_artifact_manager(Arc::clone(&artifact_manager))
        .await?;
    let runtime_driver: Arc<dyn RuntimeDriver> = driver;
    let state: Arc<dyn RuntimeStateStore> = Arc::new(FileRuntimeStateStore::new(
        root.path().join("runtime-state"),
    ));
    let runtime: Arc<dyn RuntimeClient> =
        Arc::new(ManagedRuntimeClient::new(state, runtime_driver));
    let executor = CommandExecutor::runtime_only(
        FileCommandJournal::new(root.path().join("journal"), node_id)?,
        Arc::clone(&runtime),
    )
    .with_artifacts(artifact_manager);

    let apply = RuntimeApplyRequest {
        schema: RuntimeApplyRequest::SCHEMA.into(),
        request_id: format!("runtime-buildkit-apply-{}", Uuid::now_v7()),
        deadline_at_ms: None,
        spec: spec.clone(),
    };
    let apply_command = command(
        node_id,
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(apply),
        },
    )?;
    build.dispatch(
        NodeCommandId::from_uuid(apply_command.command_id),
        build.updated_at + Duration::milliseconds(1),
    )?;
    let acknowledgement = executor.execute(apply_command).await?;
    let observation = applied_observation(acknowledgement.outcome)?;
    let resource_id = observation.provider_resource_id.clone();
    let task_diagnostics = if observation.state == RuntimeUnitState::Succeeded {
        None
    } else {
        Some(runtime_task_diagnostics(runtime.as_ref(), &spec).await)
    };

    let verification = async {
        require(
            observation.state == RuntimeUnitState::Succeeded,
            format!(
                "Runtime BuildKit Task did not succeed: {:?}; bounded Runtime logs: {}",
                observation.failure,
                task_diagnostics.as_deref().unwrap_or("not captured"),
            ),
        )?;
        require(
            observation.outputs.len() == 1,
            "Runtime BuildKit Task did not publish exactly one output Artifact",
        )?;
        let resource_id = resource_id.as_deref().ok_or_else(|| {
            std::io::Error::other("Runtime BuildKit Task omitted its Docker resource identity")
        })?;
        let container = docker.inspect_container(resource_id, None).await?;
        let host = container
            .host_config
            .ok_or_else(|| std::io::Error::other("Docker Task omitted host configuration"))?;
        require(
            host.network_mode.as_deref() == Some("none"),
            "Runtime BuildKit Task was not attached to Docker network mode none",
        )?;
        let expected_bind = format!("{expected_volume}:{BUILDKIT_SOCKET_ROOT}:ro");
        require(
            host.binds
                .unwrap_or_default()
                .iter()
                .any(|binding| binding == &expected_bind),
            format!("Runtime BuildKit Task did not mount {expected_bind:?}"),
        )?;
        let command = container
            .config
            .and_then(|config| config.cmd)
            .unwrap_or_default();
        require(
            command
                .windows(2)
                .any(|arguments| arguments == ["--opt", "force-network-mode=none"]),
            "Docker Task command omitted the BuildKit force-network-mode=none denial",
        )?;

        let output = &observation.outputs[0];
        let artifact = BuildArtifact::new(
            output.artifact.uri.clone(),
            output.artifact.digest.clone(),
            output.artifact.media_type.clone(),
            output.size_bytes,
        )?;
        build.begin_validation(
            artifact.clone(),
            build.updated_at + Duration::milliseconds(1),
        )?;
        let validator = Arc::new(RuntimeBuildOutputValidator::new(
            Arc::clone(&artifacts),
            root.path().join("validation"),
            512 * 1024 * 1024,
            100_000,
            1024 * 1024 * 1024,
            10_000,
            1024 * 1024 * 1024,
        )?);
        let validated = validator.validate(&artifact, &revision.recipe).await?;
        require(
            validated.platforms.len() == 1 && validated.platforms[0].as_str() == "linux/amd64",
            "validated Runtime BuildKit output changed the requested platform",
        )?;
        build.record_validated_output(
            validated.clone(),
            build.updated_at + Duration::milliseconds(1),
        )?;

        let registry = RegistryGate::from_environment()?;
        let publisher = OciRegistryArtifactPublisher::new(
            Arc::clone(&validator),
            StdDuration::from_secs(30),
            registry.insecure_hosts.clone(),
            OciRegistryArtifactPublisherOptions {
                registry: registry.authority.clone(),
                repository_prefix: "a3s-cloud/runtime-buildkit-gates".into(),
                credential_env: registry.credential.name().into(),
                allow_anonymous: false,
            },
        )?;
        let target = publisher.target_for(&build)?;
        build.begin_publication(target.clone(), build.updated_at + Duration::milliseconds(1))?;
        let request = OciPublicationRequest::new(target.clone(), validated)?;
        let published = publisher.publish(&request).await?;
        require(
            published.uri == target.uri(),
            "Runtime BuildKit publication changed its digest-only target",
        )?;
        require(
            publisher.find(&request).await? == Some(published.clone()),
            "Runtime BuildKit publication was not remotely verifiable",
        )?;
        require(
            publisher.publish(&request).await? == published,
            "Runtime BuildKit publication replay changed its result",
        )?;
        build.record_published_artifact(published, build.updated_at + Duration::milliseconds(1))?;
        Ok::<(), Box<dyn Error>>(())
    }
    .await;
    let credential_cleanup = require(
        std::env::var_os(PUBLICATION_CREDENTIAL_ENV).is_none(),
        "Runtime BuildKit publication credential environment was not removed",
    );

    let removal = RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("runtime-buildkit-remove-{}", Uuid::now_v7()),
        unit_id: spec.unit_id,
        generation: spec.generation,
        deadline_at_ms: None,
    };
    let remove_command = command(
        node_id,
        2,
        NodeCommandPayload::RuntimeRemove { request: removal },
    )?;
    let build_cleanup = if verification.is_ok() {
        build.begin_cleanup(
            NodeCommandId::from_uuid(remove_command.command_id),
            build.updated_at + Duration::milliseconds(1),
        )
    } else {
        Ok(())
    };
    let removal_result = executor.execute(remove_command).await;
    verification?;
    credential_cleanup?;
    build_cleanup?;
    let removal_ack = removal_result?;
    require(
        matches!(
            removal_ack.outcome,
            NodeCommandOutcome::Succeeded {
                result
            } if matches!(*result, NodeCommandResult::RuntimeRemoved { .. })
        ),
        "Runtime BuildKit Task cleanup did not return a removal receipt",
    )?;
    match docker
        .inspect_container(
            resource_id.as_deref().ok_or_else(|| {
                std::io::Error::other("Runtime BuildKit cleanup omitted its resource identity")
            })?,
            None,
        )
        .await
    {
        Err(DockerError::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        Ok(_) => {
            return Err(std::io::Error::other(
                "Runtime BuildKit Task still exists after its removal receipt",
            )
            .into())
        }
        Err(error) => {
            return Err(std::io::Error::other(format!(
                "could not verify Runtime BuildKit Task removal: {error}"
            ))
            .into())
        }
    }
    build.complete(build.updated_at + Duration::milliseconds(1))?;
    require(
        build.status == BuildRunStatus::Succeeded,
        "Runtime BuildKit publication did not complete the durable build",
    )?;
    Ok(())
}

async fn runtime_task_diagnostics(runtime: &dyn RuntimeClient, spec: &RuntimeUnitSpec) -> String {
    let query = RuntimeLogQuery {
        schema: RuntimeLogQuery::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        cursor: None,
        limit: 512,
        stream: None,
    };
    let chunks = match runtime.logs(&query).await {
        Ok(chunks) => chunks,
        Err(error) => return format!("log retrieval failed: {error}"),
    };
    if chunks.is_empty() {
        return "no Runtime log records".into();
    }
    let mut diagnostics = String::new();
    for chunk in chunks {
        diagnostics.push_str(match chunk.stream {
            RuntimeLogStream::Stdout => "[stdout] ",
            RuntimeLogStream::Stderr => "[stderr] ",
        });
        diagnostics.push_str(&chunk.data);
        if !chunk.data.ends_with('\n') {
            diagnostics.push('\n');
        }
        if diagnostics.len() >= TASK_DIAGNOSTIC_BYTES {
            break;
        }
    }
    let mut end = diagnostics.len().min(TASK_DIAGNOSTIC_BYTES);
    while !diagnostics.is_char_boundary(end) {
        end -= 1;
    }
    diagnostics.truncate(end);
    diagnostics
}

struct RegistryGate {
    authority: String,
    insecure_hosts: Vec<String>,
    credential: RegistryCredentialEnvironment,
}

impl RegistryGate {
    fn from_environment() -> Result<Self, Box<dyn Error>> {
        let url = Url::parse(&required_environment(REGISTRY_URL_ENV)?)?;
        require(
            matches!(url.scheme(), "http" | "https")
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none(),
            "Runtime BuildKit registry URL must be an HTTP(S) origin",
        )?;
        let host = url.host_str().ok_or_else(|| {
            std::io::Error::other("Runtime BuildKit registry URL omitted its host")
        })?;
        let port = url.port().ok_or_else(|| {
            std::io::Error::other("Runtime BuildKit registry URL requires an explicit port")
        })?;
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let insecure_hosts = if url.scheme() == "http" {
            vec![authority.clone()]
        } else {
            Vec::new()
        };
        let credential = RegistryCredentialEnvironment::install(
            required_environment(REGISTRY_USERNAME_ENV)?,
            required_environment(REGISTRY_PASSWORD_ENV)?,
        )?;
        Ok(Self {
            authority,
            insecure_hosts,
            credential,
        })
    }
}

struct RegistryCredentialEnvironment;

impl RegistryCredentialEnvironment {
    fn install(username: String, password: String) -> Result<Self, Box<dyn Error>> {
        require(
            std::env::var_os(PUBLICATION_CREDENTIAL_ENV).is_none(),
            "Runtime BuildKit publication credential environment is already occupied",
        )?;
        let material = serde_json::to_string(&serde_json::json!({
            "schema": RegistryCredentialMaterial::SCHEMA,
            "username": username,
            "password": password,
        }))?;
        RegistryCredentialMaterial::parse(material.as_bytes())?;
        std::env::set_var(PUBLICATION_CREDENTIAL_ENV, material);
        Ok(Self)
    }

    fn name(&self) -> &'static str {
        PUBLICATION_CREDENTIAL_ENV
    }
}

impl Drop for RegistryCredentialEnvironment {
    fn drop(&mut self) {
        std::env::remove_var(PUBLICATION_CREDENTIAL_ENV);
    }
}

fn required_environment(name: &str) -> Result<String, std::io::Error> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("required gate environment {name} is missing")))
}

fn prepared_build(
    input: BuildArtifact,
) -> Result<(crate::modules::artifacts::BuildRun, ExternalSourceRevision), String> {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let revision_id = SourceRevisionId::new();
    let accepted_at = Utc::now();
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: revision_id,
        repository: GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?,
        commit_sha: GitCommitSha::parse("a".repeat(40))?,
        recipe: BuildRecipe::dockerfile(
            BuildRecipe::SCHEMA,
            BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )?,
        accepted_at,
    })?;
    let mut build = crate::modules::artifacts::BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        revision_id,
        accepted_at,
    );
    build.begin_preparation(accepted_at + Duration::milliseconds(1))?;
    build.record_input(
        input.digest.clone(),
        input,
        accepted_at + Duration::milliseconds(2),
    )?;
    Ok((build, revision))
}

async fn admit_artifact(
    artifacts: Arc<dyn INodeArtifactStore>,
    root: &Path,
    name: &str,
    bytes: &[u8],
) -> Result<BuildArtifact, Box<dyn Error>> {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    let artifact = ArtifactRef {
        uri: artifact_uri(&digest)?,
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    };
    let descriptor = NodeArtifactDescriptor::new(artifact, bytes.len() as u64)?;
    let path = root.join(name);
    tokio::fs::write(&path, bytes).await?;
    let file = tokio::fs::File::open(&path).await?;
    let stored = artifacts.put(&descriptor, Box::pin(file)).await?;
    tokio::fs::remove_file(path).await?;
    BuildArtifact::new(
        stored.descriptor.artifact.uri,
        stored.descriptor.artifact.digest,
        stored.descriptor.artifact.media_type,
        stored.descriptor.size_bytes,
    )
    .map_err(Into::into)
}

fn directory_archive(files: &[(&str, &[u8], u32)]) -> Result<Vec<u8>, std::io::Error> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, content, mode) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(*mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder.append_data(&mut header, path, Cursor::new(content))?;
    }
    builder.into_inner()
}

fn docker(socket: &str) -> Result<Docker, Box<dyn Error>> {
    let path = socket.strip_prefix("unix://").ok_or_else(|| {
        std::io::Error::other("Runtime BuildKit gate Docker socket must use unix://")
    })?;
    Ok(Docker::connect_with_unix(path, 300, API_DEFAULT_VERSION)?)
}

fn runtime_volume_name(namespace: &str, volume_id: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(volume_id.as_bytes()));
    format!("a3s-{namespace}-volume-{}", &digest[..16])
}

fn command(
    node_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> Result<NodeCommandEnvelope, String> {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id: Uuid::now_v7(),
            issued_at,
            not_after: issued_at + Duration::minutes(6),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
}

fn applied_observation(outcome: NodeCommandOutcome) -> Result<RuntimeObservation, Box<dyn Error>> {
    match outcome {
        NodeCommandOutcome::Succeeded { result } => match *result {
            NodeCommandResult::RuntimeApplied { observation } => Ok(*observation),
            other => Err(std::io::Error::other(format!(
                "Runtime BuildKit apply returned an unexpected result: {other:?}"
            ))
            .into()),
        },
        NodeCommandOutcome::Rejected { failure } | NodeCommandOutcome::Failed { failure } => {
            Err(std::io::Error::other(format!(
                "Runtime BuildKit command failed with {}: {}",
                failure.code, failure.message
            ))
            .into())
        }
    }
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message.into()).into())
    }
}

struct LocalArtifactTransport {
    artifacts: Arc<dyn INodeArtifactStore>,
}

impl LocalArtifactTransport {
    fn new(artifacts: Arc<dyn INodeArtifactStore>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl NodeArtifactTransport for LocalArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let artifact = request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?;
        let mut opened = self
            .artifacts
            .open(&artifact)
            .await
            .map_err(transport_error)?;
        if opened.descriptor.size_bytes > maximum_bytes {
            return Err(NodeControlClientError::Invalid(
                "Runtime BuildKit input exceeds the node transfer bound".into(),
            ));
        }
        let mut destination = tokio::fs::File::create(destination)
            .await
            .map_err(io_transport_error)?;
        let copied = tokio::io::copy(&mut opened.reader, &mut destination)
            .await
            .map_err(io_transport_error)?;
        destination.flush().await.map_err(io_transport_error)?;
        if copied != opened.descriptor.size_bytes {
            return Err(NodeControlClientError::Invalid(
                "Runtime BuildKit input changed during transfer".into(),
            ));
        }
        Ok(DownloadedNodeArtifact { size_bytes: copied })
    }

    async fn upload(
        &self,
        request: &NodeArtifactUploadRequest,
        source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let artifact = ArtifactRef {
            uri: artifact_uri(&request.digest).map_err(NodeControlClientError::Invalid)?,
            digest: request.digest.clone(),
            media_type: request.media_type.clone(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, request.size_bytes)
            .map_err(NodeControlClientError::Invalid)?;
        let file = tokio::fs::File::open(source)
            .await
            .map_err(io_transport_error)?;
        let stored = self
            .artifacts
            .put(&descriptor, Box::pin(file))
            .await
            .map_err(transport_error)?;
        Ok(NodeArtifactUploadReceipt {
            schema: NodeArtifactUploadReceipt::SCHEMA.into(),
            node_id: request.node_id,
            command_id: request.command_id,
            spec_digest: request.spec_digest.clone(),
            artifact: RuntimeOutputArtifact {
                name: request.output_name.clone(),
                artifact: stored.descriptor.artifact,
                size_bytes: stored.descriptor.size_bytes,
            },
            replayed: stored.replayed,
        })
    }
}

fn transport_error(error: impl std::fmt::Display) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}

fn io_transport_error(error: std::io::Error) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}
