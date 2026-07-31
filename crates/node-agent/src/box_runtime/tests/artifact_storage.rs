use crate::{
    build_box_runtime_provider, ArtifactConfig, BoxRuntimeConfig, BoxRuntimeIsolation,
    CommandExecutor, DownloadedNodeArtifact, FileCommandJournal, NodeArtifactManager,
    NodeArtifactTransport, NodeControlClientError,
};
use a3s_box_runtime::VolumeStore;
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeCommandAck, NodeCommandEnvelope, NodeCommandMetadata,
    NodeCommandOutcome, NodeCommandPayload, NodeCommandResult, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeActionRequest,
    RuntimeApplyRequest, RuntimeMount, RuntimeMountSource, RuntimeNetworkSpec, RuntimeObservation,
    RuntimeOutputArtifact, RuntimeOutputSpec, RuntimeProcessSpec, RuntimeUnitClass,
    RuntimeUnitSpec, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

struct GateArtifactTransport {
    input: ArtifactRef,
    input_bytes: Vec<u8>,
    downloads: AtomicUsize,
    uploads: Mutex<Vec<Vec<u8>>>,
}

#[async_trait]
impl NodeArtifactTransport for GateArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?
            != self.input
            || self.input_bytes.len() as u64 > maximum_bytes
        {
            return Err(NodeControlClientError::Invalid(
                "real Box gate requested an unexpected input Artifact".into(),
            ));
        }
        tokio::fs::write(destination, &self.input_bytes)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(DownloadedNodeArtifact {
            size_bytes: self.input_bytes.len() as u64,
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
        let bytes = tokio::fs::read(source)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        if bytes.len() as u64 != request.size_bytes
            || format!("sha256:{:x}", Sha256::digest(&bytes)) != request.digest
        {
            return Err(NodeControlClientError::Invalid(
                "real Box output changed before Artifact publication".into(),
            ));
        }
        self.uploads
            .lock()
            .map_err(|_| NodeControlClientError::Transport("Artifact upload lock poisoned".into()))?
            .push(bytes);
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
            replayed: false,
        })
    }
}

#[tokio::test]
#[ignore = "requires A3S_CLOUD_TEST_BOX=1 on the dedicated real Box provider runner"]
async fn real_box_materializes_artifacts_volumes_tmpfs_and_publishes_outputs() -> TestResult<()> {
    require_gate()?;
    let home = PathBuf::from(std::env::var("A3S_HOME")?).canonicalize()?;
    let runtime_state = tempfile::tempdir()?;
    let node_state = tempfile::tempdir()?;
    let node_id = Uuid::now_v7();
    let aggregate_id = Uuid::now_v7();
    let input_bytes = directory_archive(&[("payload.txt", b"cloud-artifact-input")])?;
    let input = cloud_artifact(&input_bytes)?;
    let transport = Arc::new(GateArtifactTransport {
        input: input.clone(),
        input_bytes,
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let artifact_manager = Arc::new(
        NodeArtifactManager::new(
            node_state.path(),
            ArtifactConfig {
                max_blob_bytes: 4 * 1024 * 1024,
                max_entries: 1_000,
                max_file_bytes: 2 * 1024 * 1024,
                max_expanded_bytes: 8 * 1024 * 1024,
            },
            node_id,
            transport.clone(),
        )
        .map_err(invalid)?,
    );
    let config = BoxRuntimeConfig {
        home_dir: home.clone(),
        secret_root: home.join("runtime-secrets").canonicalize()?,
        isolation: BoxRuntimeIsolation::Sandbox,
        control_timeout_ms: 120_000,
        task_poll_interval_ms: 25,
    };
    let volume_id = format!("cloud-box-volume-{}", Uuid::now_v7().simple());
    let first_spec = task_spec(
        "writer",
        1,
        input.clone(),
        &volume_id,
        false,
        "if printf forbidden > /mnt/cloud-input/forbidden 2>/dev/null; then exit 71; fi; test \"$(cat /mnt/cloud-input/payload.txt)\" = cloud-artifact-input; test ! -e /mnt/cloud-tmpfs/token; printf ephemeral > /mnt/cloud-tmpfs/token; printf durable > /mnt/cloud-persistent/marker; printf published-cloud-output > /mnt/cloud-output/result.txt",
        true,
    )?;

    let provider = build_box_runtime_provider(&config, runtime_state.path())?;
    let runtime = provider
        .into_artifact_bound_client(artifact_manager.clone())
        .await?;
    let journal = FileCommandJournal::new(node_state.path(), node_id)?;
    let executor = CommandExecutor::runtime_only(journal, runtime.clone())
        .with_artifacts(artifact_manager.clone());
    let apply = command(
        node_id,
        aggregate_id,
        1,
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("cloud-box-storage-apply-{}", Uuid::now_v7()),
                deadline_at_ms: None,
                spec: first_spec.clone(),
            }),
            resource_claim: None,
        },
    )?;
    let applied = executor.execute(apply.clone()).await?;
    let observation = applied_observation(&applied)?;
    if observation.state != RuntimeUnitState::Succeeded
        || observation.outputs.len() != 1
        || !observation.outputs[0]
            .artifact
            .uri
            .starts_with("a3s-cloud-artifact://sha256/")
    {
        return Err(invalid("Cloud did not publish the Box Task output").into());
    }
    {
        let uploads = transport
            .uploads
            .lock()
            .map_err(|_| invalid("Artifact upload lock poisoned"))?;
        if uploads.len() != 1
            || uploaded_file(&uploads[0], "result.txt")?.as_deref()
                != Some(b"published-cloud-output")
        {
            return Err(invalid("published Task output archive changed content").into());
        }
    }
    drop(executor);
    drop(runtime);

    let recovered_provider = build_box_runtime_provider(&config, runtime_state.path())?;
    let recovered_runtime = recovered_provider
        .into_artifact_bound_client(artifact_manager.clone())
        .await?;
    let recovered_journal = FileCommandJournal::new(node_state.path(), node_id)?;
    let recovered = CommandExecutor::runtime_only(recovered_journal, recovered_runtime.clone())
        .with_artifacts(artifact_manager.clone());
    let mut replay = apply;
    replay.lease_id = Uuid::now_v7();
    if applied_observation(&recovered.execute(replay).await?)? != observation {
        return Err(invalid("journal recovery changed the published Task output").into());
    }
    expect_removed(
        &recovered
            .execute(command(
                node_id,
                aggregate_id,
                2,
                NodeCommandPayload::RuntimeRemove {
                    request: action_request("writer-remove", &first_spec),
                },
            )?)
            .await?,
    )?;

    let second_spec = task_spec(
        "reader",
        2,
        input,
        &volume_id,
        true,
        "if printf forbidden > /mnt/cloud-input/forbidden 2>/dev/null; then exit 72; fi; if printf forbidden > /mnt/cloud-persistent/forbidden 2>/dev/null; then exit 73; fi; test \"$(cat /mnt/cloud-persistent/marker)\" = durable; test ! -e /mnt/cloud-tmpfs/token; test \"$(cat /mnt/cloud-input/payload.txt)\" = cloud-artifact-input",
        false,
    )?;
    let second = recovered
        .execute(command(
            node_id,
            aggregate_id,
            3,
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("cloud-box-storage-reuse-{}", Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec: second_spec.clone(),
                }),
                resource_claim: None,
            },
        )?)
        .await?;
    if applied_observation(&second)?.state != RuntimeUnitState::Succeeded {
        return Err(invalid("recovered Box did not reuse the persistent Volume").into());
    }
    expect_removed(
        &recovered
            .execute(command(
                node_id,
                aggregate_id,
                4,
                NodeCommandPayload::RuntimeRemove {
                    request: action_request("reader-remove", &second_spec),
                },
            )?)
            .await?,
    )?;
    drop(recovered);
    drop(recovered_runtime);

    remove_gate_volume(&home, &volume_id)?;
    require_clean_state(&home, node_state.path())?;
    if transport.downloads.load(Ordering::SeqCst) != 2 {
        return Err(invalid("Artifact cleanup did not fence both Runtime specifications").into());
    }
    println!(
        "A3S_CLOUD_BOX_ARTIFACT_VOLUME_OUTPUT_CERTIFIED node_id={node_id} volume_id={volume_id}"
    );
    Ok(())
}

fn task_spec(
    role: &str,
    generation: u64,
    input: ArtifactRef,
    volume_id: &str,
    volume_read_only: bool,
    script: &str,
    output: bool,
) -> TestResult<RuntimeUnitSpec> {
    let image = std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_IMAGE")?;
    let (repository, digest) = image
        .rsplit_once('@')
        .ok_or_else(|| invalid("Box conformance image is not digest-pinned"))?;
    let spec = RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: format!("cloud-box-storage-{role}-{}", Uuid::now_v7().simple()),
        generation,
        class: RuntimeUnitClass::Task,
        artifact: ArtifactRef {
            uri: format!("oci://{repository}@{digest}"),
            digest: digest.into(),
            media_type: std::env::var("A3S_BOX_RUNTIME_CONFORMANCE_MEDIA_TYPE")?,
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/sh".into(), "-ceu".into()],
            args: vec![script.into()],
            working_directory: Some("/".into()),
            environment: BTreeMap::new(),
        },
        mounts: vec![
            RuntimeMount {
                name: "input".into(),
                source: RuntimeMountSource::Artifact { artifact: input },
                target: "/mnt/cloud-input".into(),
                read_only: true,
            },
            RuntimeMount {
                name: "persistent".into(),
                source: RuntimeMountSource::Volume {
                    volume_id: volume_id.into(),
                },
                target: "/mnt/cloud-persistent".into(),
                read_only: volume_read_only,
            },
            RuntimeMount {
                name: "scratch".into(),
                source: RuntimeMountSource::Tmpfs {
                    size_bytes: 4 * 1024 * 1024,
                },
                target: "/mnt/cloud-tmpfs".into(),
                read_only: false,
            },
        ],
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 64,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: Some(30_000),
        },
        isolation: IsolationLevel::Sandbox,
        health: None,
        restart: RestartPolicy::Never,
        outputs: output
            .then(|| RuntimeOutputSpec {
                name: "result".into(),
                path: "/mnt/cloud-output".into(),
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
                max_bytes: 4 * 1024 * 1024,
            })
            .into_iter()
            .collect(),
        semantics_profile_digest: None,
    };
    spec.validate().map_err(invalid)?;
    Ok(spec)
}

fn command(
    node_id: Uuid,
    aggregate_id: Uuid,
    sequence: u64,
    payload: NodeCommandPayload,
) -> TestResult<NodeCommandEnvelope> {
    let issued_at = Utc::now() - ChronoDuration::seconds(1);
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence,
            aggregate_id,
            issued_at,
            not_after: issued_at + ChronoDuration::minutes(30),
            correlation_id: Uuid::now_v7(),
        },
        payload,
    )
    .map_err(|error| invalid(error).into())
}

fn action_request(prefix: &str, spec: &RuntimeUnitSpec) -> RuntimeActionRequest {
    RuntimeActionRequest {
        schema: RuntimeActionRequest::SCHEMA.into(),
        request_id: format!("cloud-box-storage-{prefix}-{}", Uuid::now_v7()),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        deadline_at_ms: None,
    }
}

fn applied_observation(acknowledgement: &NodeCommandAck) -> TestResult<&RuntimeObservation> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeApplied { observation } => Ok(observation),
        result => Err(invalid(format!("unexpected apply result: {result:?}")).into()),
    }
}

fn expect_removed(acknowledgement: &NodeCommandAck) -> TestResult<()> {
    match succeeded_result(acknowledgement)? {
        NodeCommandResult::RuntimeRemoved { removal } if !removal.unit_id.is_empty() => Ok(()),
        result => Err(invalid(format!("unexpected remove result: {result:?}")).into()),
    }
}

fn succeeded_result(acknowledgement: &NodeCommandAck) -> TestResult<&NodeCommandResult> {
    match &acknowledgement.outcome {
        NodeCommandOutcome::Succeeded { result } => Ok(result),
        outcome => Err(invalid(format!("Cloud command did not succeed: {outcome:?}")).into()),
    }
}

fn cloud_artifact(bytes: &[u8]) -> TestResult<ArtifactRef> {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    Ok(ArtifactRef {
        uri: artifact_uri(&digest).map_err(invalid)?,
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    })
}

fn directory_archive(entries: &[(&str, &[u8])]) -> TestResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, path, *bytes)?;
    }
    builder.finish()?;
    Ok(builder.into_inner()?)
}

fn uploaded_file(archive: &[u8], expected: &str) -> TestResult<Option<Vec<u8>>> {
    let mut found = None;
    for entry in tar::Archive::new(archive).entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        if entry.path()?.as_ref() == Path::new(expected) {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            found = Some(bytes);
        }
    }
    Ok(found)
}

fn remove_gate_volume(home: &Path, volume_id: &str) -> TestResult<()> {
    let store = VolumeStore::new(home.join("volumes.json"), home.join("volumes"));
    let volume = store
        .list()?
        .into_iter()
        .find(|volume| {
            volume
                .labels
                .get("a3s.runtime.volume-id")
                .map(String::as_str)
                == Some(volume_id)
        })
        .ok_or_else(|| invalid("persistent Volume disappeared before cleanup"))?;
    if !volume.in_use_by.is_empty()
        || std::fs::read(Path::new(&volume.mount_point).join("marker"))? != b"durable"
    {
        return Err(invalid("persistent Volume lost its detached durable state").into());
    }
    store.remove(&volume.name, false)?;
    if !store.list()?.is_empty() {
        return Err(invalid("Box retained an output or persistent Volume after cleanup").into());
    }
    Ok(())
}

fn require_clean_state(home: &Path, node_state: &Path) -> TestResult<()> {
    let records: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("boxes.json"))?)?;
    if records.as_array().is_none_or(|records| !records.is_empty())
        || directory_has_entries(&home.join("boxes"))?
        || directory_has_entries(&node_state.join("artifacts/mounts"))?
        || directory_has_entries(&node_state.join("artifacts/outputs"))?
        || directory_has_entries(&node_state.join("artifacts/blobs/sha256"))?
        || directory_has_entries(&node_state.join("artifacts/staging"))?
    {
        return Err(invalid("Box or Cloud Artifact cleanup retained live storage state").into());
    }
    Ok(())
}

fn directory_has_entries(path: &Path) -> io::Result<bool> {
    match std::fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().transpose()?.is_some()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn require_gate() -> io::Result<()> {
    if std::env::var("A3S_CLOUD_TEST_BOX").as_deref() != Ok("1") {
        return Err(invalid(
            "dedicated Box gate did not enable real-provider tests",
        ));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
