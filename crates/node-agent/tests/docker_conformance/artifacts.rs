use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt,
    NodeArtifactUploadRequest, NodeCommandEnvelope, NodeCommandMetadata, NodeCommandPayload,
    NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE,
};
use a3s_cloud_node_agent::{
    ArtifactConfig, DownloadedNodeArtifact, NodeArtifactManager, NodeArtifactTransport,
    NodeControlClientError,
};
use a3s_runtime::contract::{
    ArtifactRef, RuntimeApplyRequest, RuntimeMountSource, RuntimeOutputArtifact, RuntimeUnitSpec,
};
use a3s_runtime::{RuntimeError, RuntimeResult};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub(crate) struct DockerConformanceArtifacts {
    node_id: Uuid,
    root: PathBuf,
    manager: Arc<NodeArtifactManager>,
    transport: Arc<ConformanceArtifactTransport>,
}

impl DockerConformanceArtifacts {
    pub(crate) fn new(state_root: &Path, node_id: Uuid) -> RuntimeResult<Self> {
        let transport = Arc::new(ConformanceArtifactTransport::default());
        let binding: Arc<dyn NodeArtifactTransport> = transport.clone();
        let manager = Arc::new(
            NodeArtifactManager::new(
                state_root,
                ArtifactConfig {
                    max_blob_bytes: 32 * 1024 * 1024,
                    max_entries: 10_000,
                    max_file_bytes: 16 * 1024 * 1024,
                    max_expanded_bytes: 64 * 1024 * 1024,
                },
                node_id,
                binding,
            )
            .map_err(RuntimeError::InvalidRequest)?,
        );
        Ok(Self {
            node_id,
            root: state_root.join("artifacts"),
            manager,
            transport,
        })
    }

    pub(crate) fn manager(&self) -> Arc<NodeArtifactManager> {
        Arc::clone(&self.manager)
    }

    pub(crate) async fn prepare_input(
        &self,
        spec: &RuntimeUnitSpec,
        mount_name: &str,
        bytes: Vec<u8>,
    ) -> RuntimeResult<()> {
        let artifact = spec
            .mounts
            .iter()
            .find(|mount| mount.name == mount_name)
            .and_then(|mount| match &mount.source {
                RuntimeMountSource::Artifact { artifact } => Some(artifact),
                RuntimeMountSource::Volume { .. } | RuntimeMountSource::Tmpfs { .. } => None,
            })
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "Docker conformance spec omits artifact mount {mount_name:?}"
                ))
            })?;
        let actual = format!("sha256:{:x}", Sha256::digest(&bytes));
        if actual != artifact.digest {
            return Err(RuntimeError::InvalidRequest(
                "Docker conformance archive does not match its Artifact identity".into(),
            ));
        }
        self.transport
            .downloads
            .write()
            .await
            .insert(artifact.digest.clone(), bytes);
        let issued_at = Utc::now();
        let command = NodeCommandEnvelope::new(
            NodeCommandMetadata {
                command_id: Uuid::now_v7(),
                lease_id: Uuid::now_v7(),
                node_id: self.node_id,
                sequence: 1,
                aggregate_id: Uuid::now_v7(),
                issued_at,
                not_after: issued_at + Duration::minutes(10),
                correlation_id: Uuid::now_v7(),
            },
            NodeCommandPayload::RuntimeApply {
                request: Box::new(RuntimeApplyRequest {
                    schema: RuntimeApplyRequest::SCHEMA.into(),
                    request_id: format!("artifact-prepare-{}", Uuid::now_v7()),
                    deadline_at_ms: None,
                    spec: spec.clone(),
                }),
            },
        )
        .map_err(RuntimeError::InvalidRequest)?;
        self.manager
            .prepare_command(&command)
            .await
            .map_err(|error| RuntimeError::Protocol(error.to_string()))
    }

    pub(crate) async fn read_blob(&self, artifact: &ArtifactRef) -> RuntimeResult<Vec<u8>> {
        let path = self.blob_path(artifact)?;
        tokio::fs::read(path)
            .await
            .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))
    }

    pub(crate) async fn tamper_blob_same_length(
        &self,
        artifact: &ArtifactRef,
    ) -> RuntimeResult<()> {
        let path = self.blob_path(artifact)?;
        let mut bytes = tokio::fs::read(&path)
            .await
            .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))?;
        let first = bytes.first_mut().ok_or_else(|| {
            RuntimeError::Protocol("Docker conformance output blob is empty".into())
        })?;
        *first ^= 0xff;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                .await
                .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))?;
        }
        tokio::fs::write(path, bytes)
            .await
            .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))
    }

    pub(crate) async fn spec_views_absent(&self, spec: &RuntimeUnitSpec) -> RuntimeResult<bool> {
        let spec_digest = spec.digest().map_err(RuntimeError::InvalidRequest)?;
        let digest = digest_hex(&spec_digest)?.to_owned();
        for path in [
            self.root.join("mounts").join(&digest),
            self.root.join("outputs").join(&digest),
        ] {
            if tokio::fs::try_exists(path)
                .await
                .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) async fn blob_absent(&self, artifact: &ArtifactRef) -> RuntimeResult<bool> {
        Ok(!tokio::fs::try_exists(self.blob_path(artifact)?)
            .await
            .map_err(|error| RuntimeError::ProviderUnavailable(error.to_string()))?)
    }

    fn blob_path(&self, artifact: &ArtifactRef) -> RuntimeResult<PathBuf> {
        artifact.validate().map_err(RuntimeError::InvalidRequest)?;
        Ok(self
            .root
            .join("blobs/sha256")
            .join(digest_hex(&artifact.digest)?))
    }
}

pub(crate) fn directory_artifact(bytes: &[u8]) -> RuntimeResult<ArtifactRef> {
    let digest = format!("sha256:{:x}", Sha256::digest(bytes));
    Ok(ArtifactRef {
        uri: artifact_uri(&digest).map_err(RuntimeError::InvalidRequest)?,
        digest,
        media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
    })
}

fn digest_hex(digest: &str) -> RuntimeResult<&str> {
    digest
        .strip_prefix("sha256:")
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| RuntimeError::Protocol("Docker conformance digest is invalid".into()))
}

#[derive(Default)]
struct ConformanceArtifactTransport {
    downloads: RwLock<BTreeMap<String, Vec<u8>>>,
}

#[async_trait]
impl NodeArtifactTransport for ConformanceArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let bytes = self
            .downloads
            .read()
            .await
            .get(&request.artifact_digest)
            .cloned()
            .ok_or_else(|| {
                NodeControlClientError::Invalid("Docker conformance artifact was not seeded".into())
            })?;
        if bytes.is_empty() || bytes.len() as u64 > maximum_bytes {
            return Err(NodeControlClientError::Invalid(
                "Docker conformance artifact exceeds its transfer bound".into(),
            ));
        }
        tokio::fs::write(destination, &bytes)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
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
        let bytes = tokio::fs::read(source)
            .await
            .map_err(|error| NodeControlClientError::Transport(error.to_string()))?;
        if bytes.len() as u64 != request.size_bytes
            || format!("sha256:{:x}", Sha256::digest(&bytes)) != request.digest
        {
            return Err(NodeControlClientError::Invalid(
                "Docker conformance upload changed output identity".into(),
            ));
        }
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
