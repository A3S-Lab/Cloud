use super::*;
use crate::{
    ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation, DownloadedNodeArtifact,
    NodeArtifactManager, NodeArtifactTransport, NodeControlClientError,
};
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeBoxBuildCacheInput, NodeBoxBuildOutput, NodeBoxBuildPhase,
    NodeBoxBuildPlan, NodeBoxBuildRequest, NodeCommandEnvelope, NodeCommandMetadata,
    NodeCommandPayload, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const BUILD_GATE_ENV: &str = "A3S_CLOUD_TEST_G0_BOX_BUILD";
const CACHE_RESET_ENV: &str = "A3S_CLOUD_TEST_G0_BOX_BUILD_ALLOW_CACHE_RESET";
const EVIDENCE_DIR_ENV: &str = "A3S_CLOUD_TEST_G0_EVIDENCE_DIR";
const CLOUD_REVISION_ENV: &str = "A3S_CLOUD_TEST_CLOUD_REVISION";
const BOX_REVISION_ENV: &str = "A3S_CLOUD_TEST_BOX_REVISION";
const PRIVATE_SOURCE_HANDOFF_ENV: &str = "A3S_CLOUD_TEST_G0_PRIVATE_SOURCE_HANDOFF";
const BOX_RELEASE_HANDOFF_DIR_ENV: &str = "A3S_CLOUD_TEST_G0_BOX_HANDOFF_DIR";
const PROBE_TEST: &str = "box_build::tests::real_box_build_post_publication_crash_probe";
const PROBE_PARENT_ENV: &str = "A3S_CLOUD_G0_BOX_BUILD_PROBE_PARENT";
const PROBE_INPUT_ENV: &str = "A3S_CLOUD_G0_BOX_BUILD_PROBE_INPUT";
const PROBE_NODE_STATE_ENV: &str = "A3S_CLOUD_G0_BOX_BUILD_PROBE_NODE_STATE";
const PROBE_TRANSPORT_ENV: &str = "A3S_CLOUD_G0_BOX_BUILD_PROBE_TRANSPORT";
const PROBE_MARKER_ENV: &str = "A3S_CLOUD_G0_BOX_BUILD_PROBE_MARKER";
const BUILD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BuildProbeInput {
    start: NodeCommandEnvelope,
    inspect: NodeCommandEnvelope,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrivateSourceHandoff {
    schema: String,
    cloud_revision: String,
    build_run_id: Uuid,
    revision: serde_json::Value,
    source_content_digest: String,
    input_artifact: HandoffBuildArtifact,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HandoffBuildArtifact {
    uri: String,
    digest: String,
    media_type: String,
    size_bytes: u64,
}

struct SourceInput {
    artifact: ArtifactRef,
    content_digest: String,
    build_run_id: Uuid,
    kind: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BoxReleaseHandoff<'a> {
    schema: &'static str,
    cloud_revision: &'a str,
    box_revision: &'a str,
    build_run_id: Uuid,
    source_content_digest: &'a str,
    source_artifact: &'a ArtifactRef,
    build_request_digest: String,
    output: &'a NodeBoxBuildOutput,
    commands: [&'a NodeCommandEnvelope; 3],
}

#[derive(Clone)]
struct FileArtifactTransport {
    root: PathBuf,
}

impl FileArtifactTransport {
    fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn blob_path(&self, digest: &str) -> Result<PathBuf, NodeControlClientError> {
        let hex = digest.strip_prefix("sha256:").ok_or_else(|| {
            NodeControlClientError::Invalid("test Artifact digest must use sha256".into())
        })?;
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(NodeControlClientError::Invalid(
                "test Artifact digest must be canonical lowercase SHA-256".into(),
            ));
        }
        Ok(self.root.join("blobs/sha256").join(hex))
    }

    async fn record_transfer<T: Serialize>(
        &self,
        kind: &str,
        request: &T,
    ) -> Result<bool, NodeControlClientError> {
        let body = serde_json::to_vec(request).map_err(|error| {
            NodeControlClientError::Invalid(format!(
                "could not encode test Artifact transfer: {error}"
            ))
        })?;
        let key = format!("{:x}", Sha256::digest(&body));
        let directory = self.root.join("transfers").join(kind);
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(transport_error)?;
        let path = directory.join(format!("{key}.json"));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&path).await {
            Ok(mut file) => {
                file.write_all(&body).await.map_err(transport_error)?;
                file.sync_all().await.map_err(transport_error)?;
                Ok(false)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let existing = tokio::fs::read(&path).await.map_err(transport_error)?;
                if existing != body {
                    return Err(NodeControlClientError::Invalid(
                        "test Artifact transfer identity was rebound".into(),
                    ));
                }
                Ok(true)
            }
            Err(error) => Err(transport_error(error)),
        }
    }

    async fn store_blob(&self, digest: &str, bytes: &[u8]) -> Result<(), NodeControlClientError> {
        let path = self.blob_path(digest)?;
        let parent = path.parent().ok_or_else(|| {
            NodeControlClientError::Transport("test Artifact blob has no parent".into())
        })?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(transport_error)?;
        match tokio::fs::read(&path).await {
            Ok(existing) if existing == bytes => return Ok(()),
            Ok(_) => {
                return Err(NodeControlClientError::Invalid(
                    "test Artifact digest was rebound to different bytes".into(),
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(transport_error(error)),
        }
        let temporary = parent.join(format!(".upload-{}.tmp", Uuid::now_v7()));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = options.open(&temporary).await.map_err(transport_error)?;
        file.write_all(bytes).await.map_err(transport_error)?;
        file.sync_all().await.map_err(transport_error)?;
        drop(file);
        tokio::fs::rename(&temporary, &path)
            .await
            .map_err(transport_error)
    }
}

#[async_trait]
impl NodeArtifactTransport for FileArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        self.record_transfer("downloads", request).await?;
        let source = self.blob_path(&request.artifact_digest)?;
        let bytes = tokio::fs::read(source).await.map_err(transport_error)?;
        if bytes.len() as u64 > maximum_bytes
            || format!("sha256:{:x}", Sha256::digest(&bytes)) != request.artifact_digest
        {
            return Err(NodeControlClientError::Invalid(
                "test Artifact download failed its admitted bound or digest".into(),
            ));
        }
        tokio::fs::write(destination, &bytes)
            .await
            .map_err(transport_error)?;
        Ok(DownloadedNodeArtifact {
            size_bytes: bytes.len() as u64,
        })
    }

    async fn upload(
        &self,
        request: &NodeArtifactUploadRequest,
        source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let bytes = tokio::fs::read(source).await.map_err(transport_error)?;
        if bytes.len() as u64 != request.size_bytes
            || format!("sha256:{:x}", Sha256::digest(&bytes)) != request.digest
        {
            return Err(NodeControlClientError::Invalid(
                "test Artifact upload changed its admitted bytes".into(),
            ));
        }
        self.store_blob(&request.digest, &bytes).await?;
        let replayed = self.record_transfer("uploads", request).await?;
        Ok(NodeArtifactUploadReceipt {
            schema: NodeArtifactUploadReceipt::SCHEMA.into(),
            node_id: request.node_id,
            command_id: request.command_id,
            spec_digest: request.spec_digest.clone(),
            artifact: RuntimeOutputArtifact {
                name: request.output_name.clone(),
                artifact: ArtifactRef {
                    uri: artifact_uri(&request.digest).map_err(NodeControlClientError::Invalid)?,
                    digest: request.digest.clone(),
                    media_type: request.media_type.clone(),
                },
                size_bytes: request.size_bytes,
            },
            replayed,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the dedicated exact Linux Box build provider gate"]
async fn real_box_native_build_replays_after_process_death_reuses_parent_cache_and_cleans(
) -> TestResult {
    require_environment(BUILD_GATE_ENV, "1")?;
    require_environment(CACHE_RESET_ENV, "1")?;
    let cloud_revision = require_git_revision(CLOUD_REVISION_ENV)?;
    let box_revision = require_git_revision(BOX_REVISION_ENV)?;
    let home = dedicated_home()?;
    let test_root = home.join("cloud-g0-box-build-conformance");
    require_absent_then_create(&test_root).await?;
    let node_state = test_root.join("node-state");
    let transport_root = test_root.join("transport");
    let transport = FileArtifactTransport::new(&transport_root);
    let evidence_dir = PathBuf::from(require_environment(EVIDENCE_DIR_ENV, "")?);
    tokio::fs::create_dir_all(&evidence_dir).await?;

    let image_references_before = image_references(&home).await?;
    let image_content_before = image_content_digests(&home).await?;
    let receipt_residue_before = build_receipt_residue(&home).await?;
    let source = seed_source(&transport, &cloud_revision).await?;
    let node_id = Uuid::now_v7();
    let first_correlation_id = Uuid::now_v7();
    let first_operation_id = build_operation_id(source.build_run_id)?;
    let first = build_request(&source.artifact, 1, &first_operation_id, None)?;
    let first_probe = BuildProbeInput {
        start: command(
            node_id,
            1,
            source.build_run_id,
            first_correlation_id,
            NodeCommandPayload::BoxBuildStart {
                request: Box::new(first.clone()),
            },
        )?,
        inspect: command(
            node_id,
            2,
            source.build_run_id,
            first_correlation_id,
            NodeCommandPayload::BoxBuildInspect {
                request: Box::new(first.clone()),
            },
        )?,
    };
    let probe_input = test_root.join("probe-input.json");
    write_durable(&probe_input, &serde_json::to_vec(&first_probe)?).await?;
    let probe_marker = test_root.join("post-publication.json");
    let mut probe = CrashProbe::start(
        &std::env::current_exe()?,
        &probe_input,
        &node_state,
        &transport_root,
        &probe_marker,
    )?;
    let first_output = wait_for_probe_marker(&probe_marker, &mut probe).await?;
    let upload_count_before_recovery = transfer_count(&transport_root, "uploads").await?;
    if upload_count_before_recovery != 2 {
        return Err(format!(
            "first Box build published {upload_count_before_recovery} transfers instead of output plus cache"
        )
        .into());
    }
    let crash_status = probe.kill_and_wait()?;
    require_killed(crash_status)?;

    let recovered = build_executor(&home, &node_state, node_id, &transport_root)?;
    let replayed_output = wait_for_success(&recovered, &first_probe.inspect, &first).await?;
    if replayed_output != first_output
        || transfer_count(&transport_root, "uploads").await? != upload_count_before_recovery
    {
        return Err("Agent restart did not replay the exact published Box build output".into());
    }
    let first_remove = remove_and_replay(
        &recovered,
        node_id,
        3,
        source.build_run_id,
        first_correlation_id,
        &first,
    )
    .await?;
    require_no_local_artifact_files(&node_state).await?;

    reset_native_cache(&home).await?;
    let parent_cache = first_output
        .caches
        .first()
        .ok_or("first Box build omitted its parent cache")?;
    if parent_cache.receipt.entry_count == 0 {
        return Err("first Box build cache did not contain a reusable native layer".into());
    }
    if parent_cache.receipt.source_digest != source.artifact.digest {
        return Err("first Box build cache changed the admitted source Artifact digest".into());
    }
    let second_build_run_id = Uuid::now_v7();
    let second_correlation_id = Uuid::now_v7();
    let second_operation_id = build_operation_id(second_build_run_id)?;
    let second = build_request(
        &source.artifact,
        2,
        &second_operation_id,
        Some(NodeBoxBuildCacheInput {
            artifact: parent_cache.artifact.artifact.clone(),
            receipt: parent_cache.receipt.clone(),
        }),
    )?;
    let second_start = command(
        node_id,
        4,
        second_build_run_id,
        second_correlation_id,
        NodeCommandPayload::BoxBuildStart {
            request: Box::new(second.clone()),
        },
    )?;
    let second_inspect = command(
        node_id,
        5,
        second_build_run_id,
        second_correlation_id,
        NodeCommandPayload::BoxBuildInspect {
            request: Box::new(second.clone()),
        },
    )?;
    let retry = build_executor(&home, &node_state, node_id, &transport_root)?;
    retry.start(&second_start, &second).await?;
    require_hydrated_native_cache(&home).await?;
    require_parent_cache_download(&transport_root, &second).await?;
    let second_output = wait_for_success(&retry, &second_inspect, &second).await?;
    let second_cache = second_output
        .caches
        .first()
        .ok_or("retry Box build omitted its native cache")?;
    if second_cache.receipt.key != parent_cache.receipt.key || second_cache.receipt.entry_count == 0
    {
        return Err("retry Box build did not preserve the immediate-parent cache identity".into());
    }
    if second_cache.receipt.source_digest != source.artifact.digest {
        return Err("retry Box build cache changed the admitted source Artifact digest".into());
    }
    if second_output.descriptor != first_output.descriptor {
        return Err("immediate-parent cache replay changed the deterministic OCI root".into());
    }
    remove_and_replay(
        &retry,
        node_id,
        6,
        second_build_run_id,
        second_correlation_id,
        &second,
    )
    .await?;
    require_no_local_artifact_files(&node_state).await?;

    if image_references(&home).await? != image_references_before {
        return Err("Box removal left an operation-owned ImageStore reference".into());
    }
    if image_content_digests(&home).await? != image_content_before {
        return Err("Box removal left operation-owned ImageStore content".into());
    }
    if build_receipt_residue(&home).await? != receipt_residue_before {
        return Err("Box removal left a build receipt, workspace, or cache export".into());
    }

    write_box_release_handoff(
        &transport,
        &cloud_revision,
        &box_revision,
        &source,
        &first,
        &first_output,
        [&first_probe.start, &first_probe.inspect, &first_remove],
    )
    .await?;

    let evidence = serde_json::json!({
        "schema": "a3s.cloud.g0-box-build-provider-evidence.v1",
        "cloudRevision": cloud_revision,
        "boxRevision": box_revision,
        "buildRunId": source.build_run_id,
        "sourceKind": source.kind,
        "sourceContentDigest": source.content_digest,
        "sourceDigest": source.artifact.digest,
        "firstOutputDigest": first_output.descriptor.digest,
        "retryOutputDigest": second_output.descriptor.digest,
        "parentCacheKey": parent_cache.receipt.key,
        "checks": {
            "postPublicationProcessDeath": true,
            "exactOutputReplay": true,
            "immediateParentCacheDownloaded": true,
            "nativeCacheHydrated": true,
            "cacheIdentityPreserved": true,
            "cacheSourceBound": true,
            "reproducibleOutput": true,
            "productionOperationIdentity": true,
            "authoritativeRemovalReplay": true,
            "operationReceiptsRemoved": true,
            "imageReferencesRestored": true,
            "imageContentRestored": true,
            "nodeArtifactsCollected": true
        }
    });
    write_durable(
        &evidence_dir.join("box-build-provider.json"),
        &serde_json::to_vec_pretty(&evidence)?,
    )
    .await?;
    tokio::fs::remove_dir_all(&test_root).await?;
    println!(
        "A3S_CLOUD_G0_BOX_BUILD_CERTIFIED cloud_revision={} box_revision={} cache_key={}",
        cloud_revision, box_revision, parent_cache.receipt.key
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "private subprocess used only by the exact Linux Box build provider gate"]
async fn real_box_build_post_publication_crash_probe() -> TestResult {
    require_environment(PROBE_PARENT_ENV, "1")?;
    let input: BuildProbeInput = serde_json::from_slice(
        &tokio::fs::read(PathBuf::from(require_environment(PROBE_INPUT_ENV, "")?)).await?,
    )?;
    let node_state = PathBuf::from(require_environment(PROBE_NODE_STATE_ENV, "")?);
    let transport_root = PathBuf::from(require_environment(PROBE_TRANSPORT_ENV, "")?);
    let marker = PathBuf::from(require_environment(PROBE_MARKER_ENV, "")?);
    let request = command_request(&input.start)?.clone();
    if command_request(&input.inspect)? != &request {
        return Err("Box build crash probe commands changed request identity".into());
    }
    let home = dedicated_home()?;
    let executor = build_executor(&home, &node_state, input.start.node_id, &transport_root)?;
    let started = executor.start(&input.start, &request).await?;
    if matches!(
        started.phase,
        NodeBoxBuildPhase::Cancelled { .. } | NodeBoxBuildPhase::Failed { .. }
    ) {
        return Err(format!("Box build crash probe failed to start: {:?}", started.phase).into());
    }
    let output = wait_for_success(&executor, &input.inspect, &request).await?;
    write_durable(&marker, &serde_json::to_vec(&output)?).await?;
    std::future::pending::<TestResult>().await
}

fn build_executor(
    home: &Path,
    node_state: &Path,
    node_id: Uuid,
    transport_root: &Path,
) -> Result<BoxBuildCommandExecutor, NodeBoxBuildError> {
    let artifacts = Arc::new(
        NodeArtifactManager::new(
            node_state,
            ArtifactConfig {
                max_blob_bytes: 64 * 1024 * 1024,
                max_entries: 10_000,
                max_file_bytes: 64 * 1024 * 1024,
                max_expanded_bytes: 128 * 1024 * 1024,
            },
            node_id,
            Arc::new(FileArtifactTransport::new(transport_root)),
        )
        .map_err(NodeBoxBuildError::State)?,
    );
    BoxBuildCommandExecutor::new(
        &BoxRuntimeConfig {
            home_dir: home.to_path_buf(),
            secret_root: home.join("runtime-secrets"),
            isolation: BoxRuntimeIsolation::Sandbox,
            control_timeout_ms: 120_000,
            task_poll_interval_ms: 25,
            sev_snp: None,
        },
        artifacts,
    )
}

async fn seed_source(
    transport: &FileArtifactTransport,
    cloud_revision: &str,
) -> TestResult<SourceInput> {
    let Some(handoff_path) = std::env::var_os(PRIVATE_SOURCE_HANDOFF_ENV).map(PathBuf::from) else {
        let archive = directory_archive(&[
            (
                "Containerfile",
                b"FROM scratch\nCOPY payload.txt /payload.txt\nLABEL org.opencontainers.image.title=\"a3s-cloud-g0\"\n",
                0o644,
            ),
            ("payload.txt", b"exact Box-native G0 build input\n", 0o644),
        ]);
        let digest = format!("sha256:{:x}", Sha256::digest(&archive));
        transport.store_blob(&digest, &archive).await?;
        return Ok(SourceInput {
            artifact: ArtifactRef {
                uri: artifact_uri(&digest)?,
                digest: digest.clone(),
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            },
            content_digest: digest,
            build_run_id: Uuid::now_v7(),
            kind: "synthetic",
        });
    };
    if !handoff_path.is_absolute() {
        return Err("private source handoff path must be absolute".into());
    }
    let handoff: PrivateSourceHandoff =
        serde_json::from_slice(&tokio::fs::read(&handoff_path).await?)?;
    if handoff.schema != "a3s.cloud.g0-private-source-handoff.v1"
        || handoff.cloud_revision != cloud_revision
        || handoff.build_run_id.is_nil()
        || handoff.source_content_digest.len() != 71
        || !handoff.source_content_digest.starts_with("sha256:")
        || handoff.revision.is_null()
    {
        return Err("private source handoff identity is invalid".into());
    }
    let input = handoff.input_artifact;
    if input.size_bytes == 0
        || input.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE
        || input.uri != artifact_uri(&input.digest)?
    {
        return Err("private source handoff Artifact is invalid".into());
    }
    let archive_path = handoff_path
        .parent()
        .ok_or("private source handoff path has no parent")?
        .join("source-input.tar");
    let archive = tokio::fs::read(&archive_path).await?;
    if archive.len() as u64 != input.size_bytes
        || format!("sha256:{:x}", Sha256::digest(&archive)) != input.digest
    {
        return Err("private source handoff archive changed before Box admission".into());
    }
    transport.store_blob(&input.digest, &archive).await?;
    Ok(SourceInput {
        artifact: ArtifactRef {
            uri: input.uri,
            digest: input.digest,
            media_type: input.media_type,
        },
        content_digest: handoff.source_content_digest,
        build_run_id: handoff.build_run_id,
        kind: "private_github",
    })
}

async fn write_box_release_handoff(
    transport: &FileArtifactTransport,
    cloud_revision: &str,
    box_revision: &str,
    source: &SourceInput,
    request: &NodeBoxBuildRequest,
    output: &NodeBoxBuildOutput,
    commands: [&NodeCommandEnvelope; 3],
) -> TestResult {
    let Some(directory) = std::env::var_os(BOX_RELEASE_HANDOFF_DIR_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    if source.kind != "private_github" || !directory.is_absolute() {
        return Err("Box release handoff requires an absolute private-source directory".into());
    }
    require_absent_then_create(&directory).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).await?;
    }
    if request.source != source.artifact
        || commands[0].sequence != 1
        || commands[1].sequence != 2
        || commands[2].sequence != 3
        || commands.iter().any(|command| {
            command.node_id != commands[0].node_id
                || command.aggregate_id != source.build_run_id
                || command.correlation_id != commands[0].correlation_id
                || command.validate().is_err()
                || command_request(command).is_err()
                || command_request(command).ok() != Some(request)
        })
    {
        return Err("Box release handoff command chain changed its source build identity".into());
    }
    let output_artifact = &output.artifact.artifact;
    let archive = tokio::fs::read(transport.blob_path(&output_artifact.digest)?).await?;
    if archive.len() as u64 != output.artifact.size_bytes
        || format!("sha256:{:x}", Sha256::digest(&archive)) != output_artifact.digest
    {
        return Err("Box release output changed before external publication handoff".into());
    }
    write_durable(&directory.join("box-output.tar"), &archive).await?;
    let handoff = BoxReleaseHandoff {
        schema: "a3s.cloud.g0-box-release-handoff.v1",
        cloud_revision,
        box_revision,
        build_run_id: source.build_run_id,
        source_content_digest: &source.content_digest,
        source_artifact: &source.artifact,
        build_request_digest: request.binding_digest()?,
        output,
        commands,
    };
    let mut encoded = serde_json::to_vec_pretty(&handoff)?;
    encoded.push(b'\n');
    write_durable(&directory.join("box-output.json"), &encoded).await?;
    #[cfg(unix)]
    for path in ["box-output.tar", "box-output.json"] {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(directory.join(path), std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    Ok(())
}

fn build_request(
    source: &ArtifactRef,
    generation: u64,
    operation_id: &str,
    cache: Option<NodeBoxBuildCacheInput>,
) -> TestResult<NodeBoxBuildRequest> {
    let architecture = target_architecture()?;
    let source_acl = format!(
        concat!(
            "build \"oci\" {{\n",
            "  cache = \"content-addressed\"\n",
            "  context = \".\"\n",
            "  file = \"Containerfile\"\n",
            "  network = \"none\"\n",
            "  platform = \"linux/{}\"\n",
            "  schema = \"a3s.box.build-plan.v1\"\n",
            "}}\n"
        ),
        architecture
    );
    let plan = a3s_box_runtime::BoxBuildPlan::parse_acl(&source_acl)?;
    let request = NodeBoxBuildRequest {
        schema: NodeBoxBuildRequest::SCHEMA.into(),
        generation,
        source: source.clone(),
        plans: vec![NodeBoxBuildPlan {
            operation_id: operation_id.into(),
            plan_acl: plan.canonical_acl()?,
            cache,
        }],
        assembly_reference: None,
        output_max_bytes: 64 * 1024 * 1024,
        cache_max_bytes: 64 * 1024 * 1024,
    };
    request.validate()?;
    Ok(request)
}

fn target_architecture() -> TestResult<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        architecture => {
            Err(format!("unsupported Box build gate architecture {architecture}").into())
        }
    }
}

fn build_operation_id(build_run_id: Uuid) -> TestResult<String> {
    Ok(format!(
        "cloud-build-{build_run_id}-linux-{}",
        target_architecture()?
    ))
}

fn command(
    node_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
    correlation_id: Uuid,
    payload: NodeCommandPayload,
) -> Result<NodeCommandEnvelope, String> {
    let issued_at = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or_else(|| "Box build command timestamp exceeds PostgreSQL precision".to_owned())?;
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(10),
            correlation_id,
        },
        payload,
    )
}

fn command_request(command: &NodeCommandEnvelope) -> TestResult<&NodeBoxBuildRequest> {
    match &command.payload {
        NodeCommandPayload::BoxBuildStart { request }
        | NodeCommandPayload::BoxBuildInspect { request }
        | NodeCommandPayload::BoxBuildCancel { request }
        | NodeCommandPayload::BoxBuildRemove { request } => Ok(request),
        _ => Err("crash probe command is not a Box build command".into()),
    }
}

async fn wait_for_success(
    executor: &BoxBuildCommandExecutor,
    command: &NodeCommandEnvelope,
    request: &NodeBoxBuildRequest,
) -> TestResult<NodeBoxBuildOutput> {
    let deadline = tokio::time::Instant::now() + BUILD_TIMEOUT;
    loop {
        match executor.inspect(command, request).await? {
            NodeBoxBuildInspection::Succeeded { output, .. } => return Ok(*output),
            NodeBoxBuildInspection::Running { .. } | NodeBoxBuildInspection::Cancelling { .. } => {}
            NodeBoxBuildInspection::Cancelled { message, .. }
            | NodeBoxBuildInspection::Failed { message, .. } => {
                return Err(format!("Box build became terminal: {message}").into())
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Box build did not complete within 120 seconds".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn remove_and_replay(
    executor: &BoxBuildCommandExecutor,
    node_id: Uuid,
    sequence: u64,
    aggregate_id: Uuid,
    correlation_id: Uuid,
    request: &NodeBoxBuildRequest,
) -> TestResult<NodeCommandEnvelope> {
    let command = command(
        node_id,
        sequence,
        aggregate_id,
        correlation_id,
        NodeCommandPayload::BoxBuildRemove {
            request: Box::new(request.clone()),
        },
    )?;
    let removed = executor.remove(&command, request).await?;
    if removed
        .operations
        .iter()
        .any(|operation| !operation.removed)
    {
        return Err("Box did not authoritatively remove every build operation".into());
    }
    let replayed = executor.remove(&command, request).await?;
    if replayed
        .operations
        .iter()
        .any(|operation| operation.removed)
        || replayed.assembly_removed
    {
        return Err("Box build removal replay reported a second mutation".into());
    }
    Ok(command)
}

async fn require_parent_cache_download(
    transport_root: &Path,
    request: &NodeBoxBuildRequest,
) -> TestResult {
    let binding = request.binding_digest()?;
    let paths = material_paths(&transport_root.join("transfers/downloads")).await?;
    let mut names = BTreeSet::new();
    for path in paths {
        let transfer: NodeArtifactDownloadRequest =
            serde_json::from_slice(&tokio::fs::read(path).await?)?;
        if transfer.spec_digest == binding {
            names.insert(transfer.mount_name);
        }
    }
    let expected = BTreeSet::from([
        "build-source".to_string(),
        request.plans[0].cache_output_name(),
    ]);
    if names != expected {
        return Err(format!("retry Box build downloads were incomplete: {names:?}").into());
    }
    Ok(())
}

async fn require_hydrated_native_cache(home: &Path) -> TestResult {
    let keys = material_paths(&home.join("buildcache/keys")).await?;
    let blobs = material_paths(&home.join("buildcache/blobs")).await?;
    if keys.is_empty() || blobs.is_empty() {
        return Err("immediate-parent Artifact did not hydrate the native Box cache".into());
    }
    Ok(())
}

async fn reset_native_cache(home: &Path) -> TestResult {
    let cache = home.join("buildcache");
    if cache.parent() != Some(home) || home.parent().is_none() {
        return Err("refusing to reset an unsafe Box build-cache path".into());
    }
    match tokio::fs::remove_dir_all(&cache).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

async fn require_no_local_artifact_files(node_state: &Path) -> TestResult {
    let files = material_paths(&node_state.join("artifacts")).await?;
    if !files.is_empty() {
        return Err(format!("Node Artifact cleanup left files: {files:?}").into());
    }
    Ok(())
}

async fn image_references(home: &Path) -> TestResult<BTreeSet<String>> {
    let path = home.join("images/index.json");
    let body = match tokio::fs::read(path).await {
        Ok(body) => body,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    let document: serde_json::Value = serde_json::from_slice(&body)?;
    let images = document
        .get("images")
        .and_then(serde_json::Value::as_array)
        .ok_or("Box ImageStore index has no images array")?;
    images
        .iter()
        .map(|image| {
            image
                .get("reference")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "Box ImageStore entry has no reference".into())
        })
        .collect()
}

async fn image_content_digests(home: &Path) -> TestResult<BTreeSet<String>> {
    let root = home.join("images/sha256");
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => return Err(error.into()),
    };
    let mut digests = BTreeSet::new();
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            return Err(format!(
                "Box ImageStore content root contains a non-directory: {}",
                entry.path().display()
            )
            .into());
        }
        let digest = entry
            .file_name()
            .into_string()
            .map_err(|_| "Box ImageStore digest is not UTF-8")?;
        digests.insert(digest);
    }
    Ok(digests)
}

async fn build_receipt_residue(home: &Path) -> TestResult<BTreeSet<PathBuf>> {
    let root = home.join("images/build-receipts/sha256");
    let scan_root = root.clone();
    Ok(tokio::task::spawn_blocking(move || collect_receipt_residue(&scan_root)).await??)
}

fn collect_receipt_residue(root: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut residue = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                residue.insert(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
                pending.push(path);
            } else if file_type.is_file() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if !name.ends_with(".lock") {
                    residue.insert(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
                }
            } else {
                return Err(io::Error::other(format!(
                    "Box build receipt root contains an unsupported path {}",
                    path.display()
                )));
            }
        }
    }
    Ok(residue)
}

async fn material_paths(root: &Path) -> TestResult<Vec<PathBuf>> {
    let root = root.to_path_buf();
    Ok(tokio::task::spawn_blocking(move || collect_material_paths(&root)).await??)
}

fn collect_material_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            } else {
                return Err(io::Error::other(format!(
                    "test state contains an unsupported path {}",
                    entry.path().display()
                )));
            }
        }
    }
    files.sort();
    Ok(files)
}

async fn transfer_count(root: &Path, kind: &str) -> TestResult<usize> {
    Ok(material_paths(&root.join("transfers").join(kind))
        .await?
        .len())
}

fn directory_archive(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_mode(*mode);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *bytes)
            .expect("append deterministic build source");
    }
    builder.finish().expect("finish deterministic build source");
    builder.into_inner().expect("build source bytes")
}

async fn write_durable(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("durable test file has no parent"))?;
    tokio::fs::create_dir_all(parent).await?;
    let temporary = parent.join(format!(".durable-{}.tmp", Uuid::now_v7()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary).await?;
    file.write_all(body).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&temporary, path).await
}

fn transport_error(error: io::Error) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}

fn dedicated_home() -> TestResult<PathBuf> {
    let home = PathBuf::from(require_environment("A3S_HOME", "")?);
    if !home.is_absolute() || home.parent().is_none() || home == Path::new("/") {
        return Err("Box build gate requires a dedicated absolute non-root A3S_HOME".into());
    }
    std::fs::create_dir_all(&home)?;
    Ok(home.canonicalize()?)
}

async fn require_absent_then_create(path: &Path) -> TestResult {
    match tokio::fs::symlink_metadata(path).await {
        Ok(_) => {
            return Err(format!(
                "dedicated Box build test path already exists: {}",
                path.display()
            )
            .into())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    tokio::fs::create_dir(path).await?;
    Ok(())
}

fn require_environment(name: &str, expected: &str) -> TestResult<String> {
    let value = std::env::var(name).map_err(|_| format!("Box build gate omitted {name}"))?;
    if !expected.is_empty() && value != expected {
        return Err(format!("Box build gate requires {name}={expected}").into());
    }
    if value.trim().is_empty() {
        return Err(format!("Box build gate requires nonempty {name}").into());
    }
    Ok(value)
}

fn require_git_revision(name: &str) -> TestResult<String> {
    let revision = require_environment(name, "")?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(
            format!("Box build gate requires {name} to be a full lowercase Git SHA").into(),
        );
    }
    Ok(revision)
}

struct CrashProbe {
    child: Option<Child>,
}

impl CrashProbe {
    fn start(
        test_binary: &Path,
        input: &Path,
        node_state: &Path,
        transport: &Path,
        marker: &Path,
    ) -> io::Result<Self> {
        let child = std::process::Command::new(test_binary)
            .arg(PROBE_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(PROBE_PARENT_ENV, "1")
            .env(PROBE_INPUT_ENV, input)
            .env(PROBE_NODE_STATE_ENV, node_state)
            .env(PROBE_TRANSPORT_ENV, transport)
            .env(PROBE_MARKER_ENV, marker)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Self { child: Some(child) })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child
            .as_mut()
            .ok_or_else(|| io::Error::other("Box build crash probe disappeared"))?
            .try_wait()
    }

    fn kill_and_wait(mut self) -> io::Result<ExitStatus> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| io::Error::other("Box build crash probe disappeared"))?;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        child.kill()?;
        child.wait()
    }
}

impl Drop for CrashProbe {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn wait_for_probe_marker(
    path: &Path,
    probe: &mut CrashProbe,
) -> TestResult<NodeBoxBuildOutput> {
    let deadline = tokio::time::Instant::now() + BUILD_TIMEOUT;
    loop {
        match tokio::fs::read(path).await {
            Ok(body) => return Ok(serde_json::from_slice(&body)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(status) = probe.try_wait()? {
            return Err(
                format!("Box build crash probe exited before publication: {status}").into(),
            );
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("Box build crash probe did not publish within 120 seconds".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn require_killed(status: ExitStatus) -> TestResult {
    if status.success() {
        return Err("Box build crash probe exited successfully instead of being killed".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() != Some(9) {
            return Err(
                format!("Box build crash probe exited with {status} instead of SIGKILL").into(),
            );
        }
    }
    Ok(())
}
