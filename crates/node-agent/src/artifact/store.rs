use super::archive::{
    extract_directory_archive, seal_directory_root, verify_directory_archive, ArchiveLimits,
};
use super::cache_gc::garbage_collect_blobs;
use super::cache_io::{
    digest_hex, is_directory, is_read_only_file, is_regular_file, local_artifact_uri, name_key,
    read_optional_json, read_required_json, remove_tree, seal_regular_file, storage,
    validate_local_artifact, verify_file, write_json_atomic, BlobReceipt, MountReceipt,
    OutputReceipt, BLOB_RECEIPT_SCHEMA, MOUNT_RECEIPT_SCHEMA, OUTPUT_RECEIPT_SCHEMA,
};
use crate::{ArtifactConfig, NodeArtifactTransport, NodeControlClientError};
use a3s_cloud_contracts::{NodeArtifactDownloadRequest, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_runtime::contract::{ArtifactRef, RuntimeMount, RuntimeOutputArtifact, RuntimeOutputSpec};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use uuid::Uuid;

pub(crate) type LocalArtifactReader = Pin<Box<dyn AsyncRead + Send + Unpin + 'static>>;

#[derive(Debug, thiserror::Error)]
pub enum NodeArtifactError {
    #[error("node artifact request is invalid: {0}")]
    Invalid(String),
    #[error("node artifact transport failed: {0}")]
    Transport(#[from] NodeControlClientError),
    #[error("node artifact cache failed integrity validation: {0}")]
    Integrity(String),
    #[error("node artifact cache storage failed: {0}")]
    Storage(String),
}

impl NodeArtifactError {
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Transport(error) if error.retryable())
            || matches!(self, Self::Storage(_))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct NodeArtifactCache {
    root: PathBuf,
    config: ArtifactConfig,
    mutation: Arc<Mutex<()>>,
}

impl NodeArtifactCache {
    pub(super) fn new(root: PathBuf, config: ArtifactConfig) -> Result<Self, String> {
        let text = root
            .to_str()
            .ok_or_else(|| "node artifact cache path must be UTF-8".to_owned())?;
        if text.trim().is_empty()
            || text.len() > 4096
            || text.contains('\0')
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err("node artifact cache path is invalid".into());
        }
        if !(1024 * 1024..=10 * 1024 * 1024 * 1024_u64).contains(&config.max_blob_bytes)
            || config.max_entries == 0
            || config.max_entries > 1_000_000
            || config.max_file_bytes == 0
            || config.max_file_bytes > config.max_expanded_bytes
            || config.max_expanded_bytes < config.max_blob_bytes
            || config.max_expanded_bytes > 20 * 1024 * 1024 * 1024_u64
        {
            return Err("node artifact cache limits are invalid".into());
        }
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|error| format!("could not resolve node artifact cache path: {error}"))?
                .join(root)
        };
        if root.as_os_str().len() > 4096 {
            return Err("node artifact cache path is invalid".into());
        }
        Ok(Self {
            root,
            config,
            mutation: Arc::new(Mutex::new(())),
        })
    }

    pub(super) async fn materialize(
        &self,
        transport: &dyn NodeArtifactTransport,
        request: &NodeArtifactDownloadRequest,
    ) -> Result<PathBuf, NodeArtifactError> {
        request.validate().map_err(NodeArtifactError::Invalid)?;
        let _guard = self.mutation.lock().await;
        self.ensure_roots().await?;
        let view = self.mount_view(&request.spec_digest, &request.mount_name)?;
        let root = view.join("root");
        let receipt_path = view.join("receipt.json");
        if let Some(receipt) = read_optional_json::<MountReceipt>(&receipt_path).await? {
            if receipt.schema == MOUNT_RECEIPT_SCHEMA
                && receipt.spec_digest == request.spec_digest
                && receipt.mount_name == request.mount_name
                && receipt.artifact == request.artifact().map_err(NodeArtifactError::Invalid)?
                && receipt.size_bytes > 0
                && receipt.entry_count > 0
                && is_directory(&root).await?
            {
                let blob = self
                    .verify_blob(
                        &receipt.artifact.digest,
                        &receipt.artifact.media_type,
                        receipt.size_bytes,
                    )
                    .await?;
                let summary = tokio::task::spawn_blocking({
                    let root = root.clone();
                    let limits = self.archive_limits();
                    move || verify_directory_archive(&blob, &root, limits)
                })
                .await
                .map_err(|error| {
                    NodeArtifactError::Storage(format!("archive verification task failed: {error}"))
                })?
                .map_err(NodeArtifactError::Integrity)?;
                if summary.entries != receipt.entry_count
                    || summary.expanded_bytes != receipt.expanded_bytes
                {
                    return Err(NodeArtifactError::Integrity(
                        "materialized artifact summary changed after admission".into(),
                    ));
                }
                return Ok(root);
            }
            return Err(NodeArtifactError::Integrity(
                "artifact mount receipt changed its durable identity".into(),
            ));
        }
        if tokio::fs::try_exists(&view).await.map_err(storage)? {
            remove_tree(view.clone()).await?;
        }
        tokio::fs::create_dir_all(&view).await.map_err(|error| {
            NodeArtifactError::Storage(format!("could not create artifact mount view: {error}"))
        })?;
        let artifact = request.artifact().map_err(NodeArtifactError::Invalid)?;
        let (blob, size_bytes) = self
            .ensure_downloaded_blob(transport, request, &artifact)
            .await?;
        let staging_root = view.join(format!(".root-{}.tmp", Uuid::now_v7()));
        let limits = self.archive_limits();
        let extraction = tokio::task::spawn_blocking({
            let blob = blob.clone();
            let staging_root = staging_root.clone();
            let root = root.clone();
            move || {
                let extraction = extract_directory_archive(&blob, &staging_root, limits)
                    .map_err(NodeArtifactError::Integrity)?;
                if let Err(error) = std::fs::rename(&staging_root, &root) {
                    let _ = std::fs::remove_dir_all(&staging_root);
                    return Err(NodeArtifactError::Storage(format!(
                        "could not commit materialized artifact root: {error}"
                    )));
                }
                if let Err(error) = seal_directory_root(&root) {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(NodeArtifactError::Storage(error));
                }
                Ok(extraction)
            }
        })
        .await
        .map_err(|error| NodeArtifactError::Storage(format!("archive task failed: {error}")))??;
        let receipt = MountReceipt {
            schema: MOUNT_RECEIPT_SCHEMA.into(),
            spec_digest: request.spec_digest.clone(),
            mount_name: request.mount_name.clone(),
            artifact,
            size_bytes,
            entry_count: extraction.entries,
            expanded_bytes: extraction.expanded_bytes,
        };
        write_json_atomic(&receipt_path, &receipt).await?;
        Ok(root)
    }

    pub(super) async fn mount_path(
        &self,
        spec_digest: &str,
        mount: &RuntimeMount,
    ) -> Result<PathBuf, NodeArtifactError> {
        let a3s_runtime::contract::RuntimeMountSource::Artifact { artifact } = &mount.source else {
            return Err(NodeArtifactError::Invalid(
                "artifact mount path requested for a non-artifact mount".into(),
            ));
        };
        if !mount.read_only {
            return Err(NodeArtifactError::Invalid(
                "artifact mounts must be read-only".into(),
            ));
        }
        let _guard = self.mutation.lock().await;
        self.ensure_roots().await?;
        let view = self.mount_view(spec_digest, &mount.name)?;
        let receipt = read_required_json::<MountReceipt>(&view.join("receipt.json")).await?;
        let root = view.join("root");
        if receipt.schema != MOUNT_RECEIPT_SCHEMA
            || receipt.spec_digest != spec_digest
            || receipt.mount_name != mount.name
            || receipt.artifact != *artifact
            || receipt.size_bytes == 0
            || receipt.entry_count == 0
            || !is_directory(&root).await?
        {
            return Err(NodeArtifactError::Integrity(
                "materialized artifact mount does not match the Runtime specification".into(),
            ));
        }
        let blob = self
            .verify_blob(&artifact.digest, &artifact.media_type, receipt.size_bytes)
            .await?;
        let summary = tokio::task::spawn_blocking({
            let root = root.clone();
            let limits = self.archive_limits();
            move || verify_directory_archive(&blob, &root, limits)
        })
        .await
        .map_err(|error| {
            NodeArtifactError::Storage(format!("archive verification task failed: {error}"))
        })?
        .map_err(NodeArtifactError::Integrity)?;
        if summary.entries != receipt.entry_count
            || summary.expanded_bytes != receipt.expanded_bytes
        {
            return Err(NodeArtifactError::Integrity(
                "materialized artifact summary changed after admission".into(),
            ));
        }
        let canonical = tokio::fs::canonicalize(&root).await.map_err(storage)?;
        if !canonical.is_absolute() {
            return Err(NodeArtifactError::Integrity(
                "materialized artifact mount path is not absolute".into(),
            ));
        }
        Ok(canonical)
    }

    pub(super) async fn capture_output(
        &self,
        spec_digest: &str,
        output: &RuntimeOutputSpec,
        reader: LocalArtifactReader,
    ) -> Result<RuntimeOutputArtifact, NodeArtifactError> {
        if output.media_type != NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE {
            return Err(NodeArtifactError::Invalid(
                "Docker Task output requires the supported directory archive media type".into(),
            ));
        }
        let _guard = self.mutation.lock().await;
        self.ensure_roots().await?;
        let receipt_path = self.output_receipt_path(spec_digest, &output.name)?;
        if let Some(receipt) = read_optional_json::<OutputReceipt>(&receipt_path).await? {
            if receipt.schema == OUTPUT_RECEIPT_SCHEMA
                && receipt.spec_digest == spec_digest
                && receipt.output.name == output.name
                && receipt.output.artifact.media_type == output.media_type
                && receipt.output.size_bytes <= output.max_bytes
            {
                validate_local_artifact(&receipt.output.artifact)?;
                self.verify_blob(
                    &receipt.output.artifact.digest,
                    &receipt.output.artifact.media_type,
                    receipt.output.size_bytes,
                )
                .await?;
                return Ok(receipt.output);
            }
            return Err(NodeArtifactError::Integrity(
                "Task output receipt changed its durable identity".into(),
            ));
        }
        let staging = self.staging_path("output");
        let (digest, size_bytes) = self
            .stage_unidentified_blob(reader, &staging, output.max_bytes)
            .await?;
        let blob = self
            .commit_staged_blob(&staging, &digest, &output.media_type, size_bytes)
            .await?;
        let artifact = RuntimeOutputArtifact {
            name: output.name.clone(),
            artifact: ArtifactRef {
                uri: local_artifact_uri(&digest)?,
                digest,
                media_type: output.media_type.clone(),
            },
            size_bytes,
        };
        validate_local_artifact(&artifact.artifact)?;
        let receipt = OutputReceipt {
            schema: OUTPUT_RECEIPT_SCHEMA.into(),
            spec_digest: spec_digest.into(),
            output: artifact.clone(),
        };
        write_json_atomic(&receipt_path, &receipt).await?;
        if !is_regular_file(&blob).await? {
            return Err(NodeArtifactError::Integrity(
                "captured Task output blob disappeared".into(),
            ));
        }
        Ok(artifact)
    }

    pub(super) async fn output_blob(
        &self,
        spec_digest: &str,
        output: &RuntimeOutputArtifact,
    ) -> Result<PathBuf, NodeArtifactError> {
        validate_local_artifact(&output.artifact)?;
        let receipt = read_required_json::<OutputReceipt>(
            &self.output_receipt_path(spec_digest, &output.name)?,
        )
        .await?;
        if receipt.schema != OUTPUT_RECEIPT_SCHEMA
            || receipt.spec_digest != spec_digest
            || receipt.output != *output
        {
            return Err(NodeArtifactError::Integrity(
                "Task output blob does not match its receipt".into(),
            ));
        }
        self.verify_blob(
            &output.artifact.digest,
            &output.artifact.media_type,
            output.size_bytes,
        )
        .await
    }

    pub(super) async fn cleanup_spec(&self, spec_digest: &str) -> Result<(), NodeArtifactError> {
        let _guard = self.mutation.lock().await;
        self.ensure_roots().await?;
        let spec = digest_hex(spec_digest)?;
        for path in [
            self.root.join("mounts").join(spec),
            self.root.join("outputs").join(spec),
        ] {
            if tokio::fs::try_exists(&path).await.map_err(storage)? {
                remove_tree(path).await?;
            }
        }
        garbage_collect_blobs(&self.root).await
    }

    async fn ensure_downloaded_blob(
        &self,
        transport: &dyn NodeArtifactTransport,
        request: &NodeArtifactDownloadRequest,
        artifact: &ArtifactRef,
    ) -> Result<(PathBuf, u64), NodeArtifactError> {
        if let Some(receipt) = self.blob_receipt(&artifact.digest).await? {
            if receipt.media_type != artifact.media_type {
                return Err(NodeArtifactError::Integrity(
                    "artifact digest was cached with a different media type".into(),
                ));
            }
            let path = self
                .verify_blob(&artifact.digest, &artifact.media_type, receipt.size_bytes)
                .await?;
            return Ok((path, receipt.size_bytes));
        }
        let staging = self.staging_path("download");
        let downloaded = match transport
            .download(request, &staging, self.config.max_blob_bytes)
            .await
        {
            Ok(downloaded) => downloaded,
            Err(error) => {
                let _ = tokio::fs::remove_file(&staging).await;
                return Err(error.into());
            }
        };
        let path = self
            .commit_staged_blob(
                &staging,
                &artifact.digest,
                &artifact.media_type,
                downloaded.size_bytes,
            )
            .await?;
        Ok((path, downloaded.size_bytes))
    }

    async fn stage_unidentified_blob(
        &self,
        mut reader: LocalArtifactReader,
        staging: &Path,
        maximum_bytes: u64,
    ) -> Result<(String, u64), NodeArtifactError> {
        if maximum_bytes == 0 || maximum_bytes > self.config.max_blob_bytes {
            return Err(NodeArtifactError::Invalid(
                "Task output size bound exceeds node artifact limits".into(),
            ));
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(staging)
            .await
            .map_err(storage)?;
        let result = async {
            let mut digest = Sha256::new();
            let mut size = 0_u64;
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer).await.map_err(storage)?;
                if read == 0 {
                    break;
                }
                size = size.checked_add(read as u64).ok_or_else(|| {
                    NodeArtifactError::Invalid("Task output size overflowed".into())
                })?;
                if size > maximum_bytes {
                    return Err(NodeArtifactError::Invalid(
                        "Task output exceeds its maximum size".into(),
                    ));
                }
                digest.update(&buffer[..read]);
                file.write_all(&buffer[..read]).await.map_err(storage)?;
            }
            if size == 0 {
                return Err(NodeArtifactError::Invalid(
                    "Task output archive is empty".into(),
                ));
            }
            file.sync_all().await.map_err(storage)?;
            Ok((format!("sha256:{:x}", digest.finalize()), size))
        }
        .await;
        drop(file);
        if result.is_err() {
            let _ = tokio::fs::remove_file(staging).await;
        }
        result
    }

    async fn commit_staged_blob(
        &self,
        staging: &Path,
        digest: &str,
        media_type: &str,
        size_bytes: u64,
    ) -> Result<PathBuf, NodeArtifactError> {
        if size_bytes == 0 || size_bytes > self.config.max_blob_bytes {
            let _ = tokio::fs::remove_file(staging).await;
            return Err(NodeArtifactError::Integrity(
                "staged artifact size exceeds node cache bounds".into(),
            ));
        }
        if !verify_file(staging, digest, size_bytes).await? {
            let _ = tokio::fs::remove_file(staging).await;
            return Err(NodeArtifactError::Integrity(
                "staged artifact bytes do not match their typed identity".into(),
            ));
        }
        let result = self
            .commit_verified_staged_blob(staging, digest, media_type, size_bytes)
            .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(staging).await;
        }
        result
    }

    async fn commit_verified_staged_blob(
        &self,
        staging: &Path,
        digest: &str,
        media_type: &str,
        size_bytes: u64,
    ) -> Result<PathBuf, NodeArtifactError> {
        let path = self.blob_path(digest)?;
        if let Some(receipt) = self.blob_receipt(digest).await? {
            if receipt.media_type != media_type || receipt.size_bytes != size_bytes {
                return Err(NodeArtifactError::Integrity(
                    "artifact blob identity conflicts with its cache receipt".into(),
                ));
            }
            let _ = tokio::fs::remove_file(staging).await;
            return self.verify_blob(digest, media_type, size_bytes).await;
        }
        if tokio::fs::try_exists(&path).await.map_err(storage)? {
            let verified = verify_file(&path, digest, size_bytes).await?;
            if !verified {
                return Err(NodeArtifactError::Integrity(
                    "orphan artifact blob does not match its identity".into(),
                ));
            }
            let _ = tokio::fs::remove_file(staging).await;
        } else {
            tokio::fs::rename(staging, &path).await.map_err(|error| {
                NodeArtifactError::Storage(format!("could not commit artifact blob: {error}"))
            })?;
        }
        seal_regular_file(&path).await?;
        let receipt = BlobReceipt {
            schema: BLOB_RECEIPT_SCHEMA.into(),
            digest: digest.into(),
            media_type: media_type.into(),
            size_bytes,
        };
        write_json_atomic(&self.blob_receipt_path(digest)?, &receipt).await?;
        Ok(path)
    }

    async fn verify_blob(
        &self,
        digest: &str,
        media_type: &str,
        size_bytes: u64,
    ) -> Result<PathBuf, NodeArtifactError> {
        let receipt = self.blob_receipt(digest).await?.ok_or_else(|| {
            NodeArtifactError::Integrity("artifact blob receipt is missing".into())
        })?;
        if receipt.schema != BLOB_RECEIPT_SCHEMA
            || receipt.digest != digest
            || receipt.media_type != media_type
            || receipt.size_bytes != size_bytes
            || size_bytes == 0
            || size_bytes > self.config.max_blob_bytes
        {
            return Err(NodeArtifactError::Integrity(
                "artifact blob receipt is invalid".into(),
            ));
        }
        let path = self.blob_path(digest)?;
        if !verify_file(&path, digest, size_bytes).await? || !is_read_only_file(&path).await? {
            return Err(NodeArtifactError::Integrity(
                "artifact blob bytes or permissions changed after admission".into(),
            ));
        }
        Ok(path)
    }

    async fn blob_receipt(&self, digest: &str) -> Result<Option<BlobReceipt>, NodeArtifactError> {
        read_optional_json(&self.blob_receipt_path(digest)?).await
    }

    async fn ensure_roots(&self) -> Result<(), NodeArtifactError> {
        for path in [
            self.root.join("blobs/sha256"),
            self.root.join("blob-receipts/sha256"),
            self.root.join("mounts"),
            self.root.join("outputs"),
            self.root.join("staging"),
        ] {
            tokio::fs::create_dir_all(path).await.map_err(storage)?;
        }
        Ok(())
    }

    fn blob_path(&self, digest: &str) -> Result<PathBuf, NodeArtifactError> {
        Ok(self.root.join("blobs/sha256").join(digest_hex(digest)?))
    }

    fn blob_receipt_path(&self, digest: &str) -> Result<PathBuf, NodeArtifactError> {
        Ok(self
            .root
            .join("blob-receipts/sha256")
            .join(format!("{}.json", digest_hex(digest)?)))
    }

    fn mount_view(&self, spec_digest: &str, name: &str) -> Result<PathBuf, NodeArtifactError> {
        Ok(self
            .root
            .join("mounts")
            .join(digest_hex(spec_digest)?)
            .join(name_key(name)?))
    }

    fn output_receipt_path(
        &self,
        spec_digest: &str,
        name: &str,
    ) -> Result<PathBuf, NodeArtifactError> {
        Ok(self
            .root
            .join("outputs")
            .join(digest_hex(spec_digest)?)
            .join(format!("{}.json", name_key(name)?)))
    }

    fn staging_path(&self, prefix: &str) -> PathBuf {
        self.root
            .join("staging")
            .join(format!("{prefix}-{}.tmp", Uuid::now_v7()))
    }

    fn archive_limits(&self) -> ArchiveLimits {
        ArchiveLimits {
            max_entries: self.config.max_entries,
            max_file_bytes: self.config.max_file_bytes,
            max_expanded_bytes: self.config.max_expanded_bytes,
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn relative_cache_roots_are_resolved_for_docker_bind_mounts() {
        let cache = NodeArtifactCache::new(
            PathBuf::from(".a3s/test-node-artifacts"),
            ArtifactConfig {
                max_blob_bytes: 1024 * 1024,
                max_entries: 100,
                max_file_bytes: 1024 * 1024,
                max_expanded_bytes: 2 * 1024 * 1024,
            },
        )
        .expect("relative cache root");

        assert!(cache.root.is_absolute());
    }
}
