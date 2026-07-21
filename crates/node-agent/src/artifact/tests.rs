use super::*;
use crate::{ArtifactConfig, NodeControlClientError};
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandPayload,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_runtime::contract::{
    ArtifactRef, IsolationLevel, NetworkMode, ResourceLimits, RestartPolicy, RuntimeApplyRequest,
    RuntimeMount, RuntimeMountSource, RuntimeNetworkSpec, RuntimeObservation,
    RuntimeOutputArtifact, RuntimeOutputSpec, RuntimeProcessSpec, RuntimeUnitClass,
    RuntimeUnitSpec, RuntimeUnitState,
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

struct FakeTransport {
    archive: Vec<u8>,
    downloads: AtomicUsize,
    uploads: Mutex<Vec<NodeArtifactUploadRequest>>,
}

#[async_trait]
impl NodeArtifactTransport for FakeTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        if self.archive.len() as u64 > maximum_bytes {
            return Err(NodeControlClientError::Invalid(
                "test archive exceeds maximum".into(),
            ));
        }
        self.downloads.fetch_add(1, Ordering::SeqCst);
        tokio::fs::write(destination, &self.archive)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        Ok(DownloadedNodeArtifact {
            size_bytes: self.archive.len() as u64,
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
                "test upload source changed identity".into(),
            ));
        }
        self.uploads
            .lock()
            .map_err(|_| NodeControlClientError::Transport("upload lock poisoned".into()))?
            .push(request.clone());
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
async fn input_materialization_is_read_only_durable_and_replayed_without_download() {
    let directory = tempfile::tempdir().expect("artifact state");
    let archive = directory_archive(&[("source/main.sh", b"#!/bin/sh\necho ok\n", 0o755)]);
    let input = cloud_artifact(&archive);
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport {
        archive,
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let spec = task_spec(Some(input), false);
    let command = command(node_id, spec.clone());
    let manager = build_manager(directory.path(), node_id, transport.clone());

    manager
        .prepare_command(&command)
        .await
        .expect("materialize input");
    let mount = spec.mounts.first().expect("artifact mount");
    let path = manager.mount_path(&spec, mount).await.expect("mount path");
    assert_eq!(
        tokio::fs::read(path.join("source/main.sh"))
            .await
            .expect("materialized file"),
        b"#!/bin/sh\necho ok\n"
    );
    assert!(tokio::fs::metadata(path.join("source/main.sh"))
        .await
        .expect("file metadata")
        .permissions()
        .readonly());
    assert_eq!(transport.downloads.load(Ordering::SeqCst), 1);
    let digest = match &mount.source {
        RuntimeMountSource::Artifact { artifact } => artifact
            .digest
            .strip_prefix("sha256:")
            .expect("SHA-256 digest"),
        RuntimeMountSource::Volume { .. } | RuntimeMountSource::Tmpfs { .. } => {
            panic!("expected Artifact mount")
        }
    };
    assert!(
        tokio::fs::metadata(directory.path().join("artifacts/blobs/sha256").join(digest))
            .await
            .expect("blob metadata")
            .permissions()
            .readonly()
    );

    let restarted = build_manager(directory.path(), node_id, transport.clone());
    restarted
        .prepare_command(&command)
        .await
        .expect("replay materialization after restart");
    assert_eq!(transport.downloads.load(Ordering::SeqCst), 1);
    assert_eq!(
        restarted
            .mount_path(&spec, mount)
            .await
            .expect("replayed mount path"),
        path
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let file = path.join("source/main.sh");
        tokio::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))
            .await
            .expect("make materialized file writable for tamper test");
        tokio::fs::write(&file, b"#!/bin/sh\necho no\n")
            .await
            .expect("tamper materialized file");
        let tampered = build_manager(directory.path(), node_id, transport.clone());
        assert!(matches!(
            tampered.prepare_command(&command).await,
            Err(NodeArtifactError::Integrity(message))
                if message.contains("materialized artifact file")
        ));
    }
    restarted
        .cleanup_spec(&spec.digest().expect("spec digest"))
        .await
        .expect("cleanup materialized input");
}

#[tokio::test]
async fn task_output_identity_survives_restart_and_publishes_exactly() {
    let directory = tempfile::tempdir().expect("artifact state");
    let archive = directory_archive(&[("oci-layout/index.json", b"{}", 0o644)]);
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport {
        archive: Vec::new(),
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let spec = task_spec(None, true);
    let command = command(node_id, spec.clone());
    let manager = build_manager(directory.path(), node_id, transport.clone());
    let output_spec = spec.outputs.first().expect("output spec");
    let local = manager
        .capture_output(&spec, output_spec, Box::pin(Cursor::new(archive.clone())))
        .await
        .expect("capture output");
    let observation = succeeded_observation(&spec, local.clone());
    let published = manager
        .publish_command_outputs(&command, &observation)
        .await
        .expect("publish output");
    assert_eq!(published.outputs.len(), 1);
    assert_eq!(published.outputs[0].name, output_spec.name);
    assert_eq!(published.outputs[0].artifact.digest, local.artifact.digest);
    assert!(published.outputs[0]
        .artifact
        .uri
        .starts_with("a3s-cloud-artifact://sha256/"));
    assert_eq!(transport.uploads.lock().expect("uploads").len(), 1);

    let restarted = build_manager(directory.path(), node_id, transport);
    let replayed = restarted
        .capture_output(
            &spec,
            output_spec,
            Box::pin(Cursor::new(
                b"different bytes are ignored on exact replay".to_vec(),
            )),
        )
        .await
        .expect("replay captured output");
    assert_eq!(replayed, local);
    restarted
        .cleanup_spec(&spec.digest().expect("spec digest"))
        .await
        .expect("cleanup spec artifacts");
    assert!(directory_is_empty(&directory.path().join("artifacts/blobs/sha256")).await);
    assert!(directory_is_empty(&directory.path().join("artifacts/blob-receipts/sha256")).await);
    assert!(restarted
        .mount_path(
            &spec,
            &RuntimeMount {
                name: "missing".into(),
                source: RuntimeMountSource::Artifact {
                    artifact: cloud_artifact(&archive)
                },
                target: "/missing".into(),
                read_only: true,
            }
        )
        .await
        .is_err());
}

#[tokio::test]
async fn downloaded_bytes_are_verified_independently_of_the_transport() {
    let directory = tempfile::tempdir().expect("artifact state");
    let expected = directory_archive(&[("source/expected", b"expected", 0o644)]);
    let tampered = directory_archive(&[("source/tampered", b"tampered", 0o644)]);
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport {
        archive: tampered,
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let spec = task_spec(Some(cloud_artifact(&expected)), false);
    let command = command(node_id, spec);
    let manager = build_manager(directory.path(), node_id, transport);

    assert!(matches!(
        manager.prepare_command(&command).await,
        Err(NodeArtifactError::Integrity(message))
            if message.contains("staged artifact bytes")
    ));
    assert!(directory_is_empty(&directory.path().join("artifacts/staging")).await);
}

#[tokio::test]
async fn cleanup_collects_only_blobs_without_live_spec_references() {
    let directory = tempfile::tempdir().expect("artifact state");
    let archive = directory_archive(&[("source/input", b"shared", 0o444)]);
    let input = cloud_artifact(&archive);
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport {
        archive,
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let first = task_spec(Some(input.clone()), false);
    let mut second = task_spec(Some(input), false);
    second.unit_id = "second-build-task".into();
    let first_command = command(node_id, first.clone());
    let second_command = command(node_id, second.clone());
    let manager = build_manager(directory.path(), node_id, transport.clone());
    manager
        .prepare_command(&first_command)
        .await
        .expect("first materialization");
    manager
        .prepare_command(&second_command)
        .await
        .expect("second materialization");
    assert_eq!(transport.downloads.load(Ordering::SeqCst), 1);

    manager
        .cleanup_spec(&first.digest().expect("first digest"))
        .await
        .expect("clean first spec");
    let second_mount = second.mounts.first().expect("second mount");
    manager
        .mount_path(&second, second_mount)
        .await
        .expect("shared blob remains referenced");
    assert!(!directory_is_empty(&directory.path().join("artifacts/blobs/sha256")).await);

    manager
        .cleanup_spec(&second.digest().expect("second digest"))
        .await
        .expect("clean second spec");
    assert!(directory_is_empty(&directory.path().join("artifacts/blobs/sha256")).await);
}

#[tokio::test]
async fn runtime_cannot_claim_an_existing_cloud_artifact_without_local_capture() {
    let directory = tempfile::tempdir().expect("artifact state");
    let bytes = b"forged cloud output";
    let node_id = Uuid::now_v7();
    let transport = Arc::new(FakeTransport {
        archive: Vec::new(),
        downloads: AtomicUsize::new(0),
        uploads: Mutex::new(Vec::new()),
    });
    let spec = task_spec(None, true);
    let command = command(node_id, spec.clone());
    let output = RuntimeOutputArtifact {
        name: spec.outputs[0].name.clone(),
        artifact: cloud_artifact(bytes),
        size_bytes: bytes.len() as u64,
    };
    let observation = succeeded_observation(&spec, output);
    let manager = build_manager(directory.path(), node_id, transport);

    assert!(matches!(
        manager
            .publish_command_outputs(&command, &observation)
            .await,
        Err(NodeArtifactError::Integrity(_))
    ));
}

fn build_manager(root: &Path, node_id: Uuid, transport: Arc<FakeTransport>) -> NodeArtifactManager {
    NodeArtifactManager::new(root, artifact_config(), node_id, transport).expect("artifact manager")
}

fn artifact_config() -> ArtifactConfig {
    ArtifactConfig {
        max_blob_bytes: 1024 * 1024,
        max_entries: 100,
        max_file_bytes: 512 * 1024,
        max_expanded_bytes: 2 * 1024 * 1024,
    }
}

fn cloud_artifact(bytes: &[u8]) -> ArtifactRef {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    ArtifactRef {
        uri: artifact_uri(&digest).expect("artifact URI"),
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    }
}

fn directory_archive(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(*mode);
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *bytes)
            .expect("append archive file");
    }
    builder.finish().expect("finish archive");
    builder.into_inner().expect("archive bytes")
}

async fn directory_is_empty(path: &Path) -> bool {
    let mut entries = tokio::fs::read_dir(path).await.expect("read directory");
    entries
        .next_entry()
        .await
        .expect("read directory entry")
        .is_none()
}

fn task_spec(input: Option<ArtifactRef>, output: bool) -> RuntimeUnitSpec {
    RuntimeUnitSpec {
        schema: RuntimeUnitSpec::SCHEMA.into(),
        unit_id: "build-task".into(),
        generation: 1,
        class: RuntimeUnitClass::Task,
        artifact: ArtifactRef {
            uri: format!("oci://registry.example/build@sha256:{}", "c".repeat(64)),
            digest: format!("sha256:{}", "c".repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        },
        process: RuntimeProcessSpec {
            command: vec!["/bin/build".into()],
            args: Vec::new(),
            working_directory: Some("/workspace".into()),
            environment: BTreeMap::new(),
        },
        mounts: input
            .map(|artifact| RuntimeMount {
                name: "source".into(),
                source: RuntimeMountSource::Artifact { artifact },
                target: "/workspace".into(),
                read_only: true,
            })
            .into_iter()
            .collect(),
        secrets: Vec::new(),
        network: RuntimeNetworkSpec {
            mode: NetworkMode::None,
            ports: Vec::new(),
        },
        resources: ResourceLimits {
            cpu_millis: 500,
            memory_bytes: 128 * 1024 * 1024,
            pids: 128,
            ephemeral_storage_bytes: None,
            execution_timeout_ms: Some(60_000),
        },
        isolation: IsolationLevel::Container,
        health: None,
        restart: RestartPolicy::Never,
        outputs: output
            .then(|| RuntimeOutputSpec {
                name: "oci-layout".into(),
                path: "/outputs/oci-layout".into(),
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
                max_bytes: 1024 * 1024,
            })
            .into_iter()
            .collect(),
        semantics_profile_digest: None,
    }
}

fn command(node_id: Uuid, spec: RuntimeUnitSpec) -> NodeCommandEnvelope {
    let issued_at = Utc::now();
    NodeCommandEnvelope::new(
        NodeCommandMetadata {
            command_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            node_id,
            sequence: 1,
            aggregate_id: Uuid::now_v7(),
            issued_at,
            not_after: issued_at + Duration::minutes(10),
            correlation_id: Uuid::now_v7(),
        },
        NodeCommandPayload::RuntimeApply {
            request: Box::new(RuntimeApplyRequest {
                schema: RuntimeApplyRequest::SCHEMA.into(),
                request_id: format!("apply-{}", Uuid::now_v7()),
                deadline_at_ms: None,
                spec,
            }),
        },
    )
    .expect("artifact command")
}

fn succeeded_observation(
    spec: &RuntimeUnitSpec,
    output: RuntimeOutputArtifact,
) -> RuntimeObservation {
    let observation = RuntimeObservation {
        schema: RuntimeObservation::SCHEMA.into(),
        unit_id: spec.unit_id.clone(),
        generation: spec.generation,
        spec_digest: spec.digest().expect("spec digest"),
        class: RuntimeUnitClass::Task,
        state: RuntimeUnitState::Succeeded,
        provider_resource_id: Some("container-1".into()),
        provider_build: Some("docker-test".into()),
        observed_at_ms: 2,
        started_at_ms: Some(1),
        finished_at_ms: Some(2),
        health: None,
        outputs: vec![output],
        usage: None,
        evidence: None,
        provider_attestation: None,
        failure: None,
    };
    observation
        .validate_against(spec)
        .expect("succeeded observation");
    observation
}
