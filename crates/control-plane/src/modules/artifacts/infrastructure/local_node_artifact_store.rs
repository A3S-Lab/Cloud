use crate::modules::artifacts::domain::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactReader, NodeArtifactStoreError,
    NodeArtifactWrite, OpenNodeArtifact,
};
use a3s_cloud_contracts::validate_cloud_artifact;
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::path::{Component, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

const RECEIPT_SCHEMA: &str = "a3s.cloud.node-artifact-object.v1";

#[derive(Debug, Clone)]
pub struct LocalNodeArtifactStore {
    root: PathBuf,
    maximum_blob_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactReceipt {
    schema: String,
    artifact: ArtifactRef,
    size_bytes: u64,
}

impl LocalNodeArtifactStore {
    pub fn new(root: impl Into<PathBuf>, maximum_blob_bytes: u64) -> Result<Self, String> {
        let root = root.into();
        let text = root
            .to_str()
            .ok_or_else(|| "node artifact store path must be UTF-8".to_owned())?;
        if text.trim().is_empty()
            || text.len() > 4096
            || text.contains('\0')
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            || maximum_blob_bytes == 0
        {
            return Err("node artifact store options are invalid".into());
        }
        Ok(Self {
            root,
            maximum_blob_bytes,
        })
    }

    fn digest_hex<'a>(&self, digest: &'a str) -> Result<&'a str, NodeArtifactStoreError> {
        digest.strip_prefix("sha256:").ok_or_else(|| {
            NodeArtifactStoreError::Invalid("artifact digest must use sha256".into())
        })
    }

    fn blob_path(&self, digest: &str) -> Result<PathBuf, NodeArtifactStoreError> {
        let hex = self.digest_hex(digest)?;
        Ok(self.root.join("blobs").join("sha256").join(hex))
    }

    fn receipt_path(&self, digest: &str) -> Result<PathBuf, NodeArtifactStoreError> {
        let hex = self.digest_hex(digest)?;
        Ok(self
            .root
            .join("receipts")
            .join("sha256")
            .join(format!("{hex}.json")))
    }

    async fn ensure_directories(&self) -> Result<(), NodeArtifactStoreError> {
        for path in [
            self.root.join("blobs/sha256"),
            self.root.join("receipts/sha256"),
            self.root.join("staging"),
        ] {
            tokio::fs::create_dir_all(&path).await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!(
                    "could not create artifact directory {}: {error}",
                    path.display()
                ))
            })?;
        }
        Ok(())
    }

    async fn lock(&self) -> Result<std::fs::File, NodeArtifactStoreError> {
        let path = self.root.join("store.lock");
        tokio::task::spawn_blocking(move || {
            let file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(&path)
                .map_err(|error| {
                    NodeArtifactStoreError::Storage(format!(
                        "could not open artifact store lock {}: {error}",
                        path.display()
                    ))
                })?;
            file.lock_exclusive().map_err(|error| {
                NodeArtifactStoreError::Storage(format!(
                    "could not lock artifact store {}: {error}",
                    path.display()
                ))
            })?;
            Ok(file)
        })
        .await
        .map_err(|error| {
            NodeArtifactStoreError::Storage(format!("artifact lock task failed: {error}"))
        })?
    }

    async fn stage(
        &self,
        descriptor: &NodeArtifactDescriptor,
        mut reader: NodeArtifactReader,
    ) -> Result<PathBuf, NodeArtifactStoreError> {
        let path = self.root.join("staging").join(Uuid::now_v7().to_string());
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await
            .map_err(|error| {
                NodeArtifactStoreError::Storage(format!(
                    "could not create artifact staging file: {error}"
                ))
            })?;
        let result = async {
            let mut digest = Sha256::new();
            let mut size = 0_u64;
            let mut buffer = vec![0_u8; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer).await.map_err(|error| {
                    NodeArtifactStoreError::Storage(format!(
                        "could not read artifact upload: {error}"
                    ))
                })?;
                if read == 0 {
                    break;
                }
                size = size.checked_add(read as u64).ok_or_else(|| {
                    NodeArtifactStoreError::Invalid("artifact upload size overflowed".into())
                })?;
                if size > descriptor.size_bytes || size > self.maximum_blob_bytes {
                    return Err(NodeArtifactStoreError::Invalid(
                        "artifact upload exceeds its declared or configured size".into(),
                    ));
                }
                digest.update(&buffer[..read]);
                file.write_all(&buffer[..read]).await.map_err(|error| {
                    NodeArtifactStoreError::Storage(format!(
                        "could not write artifact upload: {error}"
                    ))
                })?;
            }
            if size != descriptor.size_bytes {
                return Err(NodeArtifactStoreError::Integrity(
                    "artifact upload size does not match its declaration".into(),
                ));
            }
            let actual = format!("sha256:{:x}", digest.finalize());
            if actual != descriptor.artifact.digest {
                return Err(NodeArtifactStoreError::Integrity(
                    "artifact upload digest does not match its declaration".into(),
                ));
            }
            file.sync_all().await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!("could not sync artifact upload: {error}"))
            })?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(error) = result {
            let _ = tokio::fs::remove_file(&path).await;
            return Err(error);
        }
        Ok(path)
    }

    async fn stored_descriptor(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<Option<NodeArtifactDescriptor>, NodeArtifactStoreError> {
        let receipt_path = self.receipt_path(&artifact.digest)?;
        let blob_path = self.blob_path(&artifact.digest)?;
        let receipt =
            match tokio::fs::read(&receipt_path).await {
                Ok(bytes) => Some(serde_json::from_slice::<ArtifactReceipt>(&bytes).map_err(
                    |_| NodeArtifactStoreError::Integrity("artifact receipt is invalid".into()),
                )?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(NodeArtifactStoreError::Storage(format!(
                        "could not read artifact receipt: {error}"
                    )))
                }
            };
        let metadata = match tokio::fs::metadata(&blob_path).await {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(NodeArtifactStoreError::Storage(format!(
                    "could not inspect artifact blob: {error}"
                )))
            }
        };
        match (receipt, metadata) {
            (None, None) => Ok(None),
            (Some(receipt), Some(metadata)) => {
                if receipt.schema != RECEIPT_SCHEMA
                    || receipt.artifact != *artifact
                    || receipt.size_bytes == 0
                    || receipt.size_bytes > self.maximum_blob_bytes
                    || receipt.size_bytes != metadata.len()
                    || !metadata.is_file()
                {
                    return Err(NodeArtifactStoreError::Integrity(
                        "artifact receipt does not match its blob".into(),
                    ));
                }
                self.verify_blob(&blob_path, &receipt).await?;
                Ok(Some(
                    NodeArtifactDescriptor::new(receipt.artifact, receipt.size_bytes)
                        .map_err(NodeArtifactStoreError::Integrity)?,
                ))
            }
            _ => Err(NodeArtifactStoreError::Integrity(
                "artifact blob and receipt are incomplete".into(),
            )),
        }
    }

    async fn verify_blob(
        &self,
        path: &std::path::Path,
        receipt: &ArtifactReceipt,
    ) -> Result<(), NodeArtifactStoreError> {
        let mut file = tokio::fs::File::open(path).await.map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not open artifact blob: {error}"))
        })?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!("could not verify artifact blob: {error}"))
            })?;
            if read == 0 {
                break;
            }
            size = size.checked_add(read as u64).ok_or_else(|| {
                NodeArtifactStoreError::Integrity("artifact blob size overflowed".into())
            })?;
            if size > receipt.size_bytes {
                return Err(NodeArtifactStoreError::Integrity(
                    "artifact blob changed after admission".into(),
                ));
            }
            digest.update(&buffer[..read]);
        }
        if size != receipt.size_bytes
            || format!("sha256:{:x}", digest.finalize()) != receipt.artifact.digest
        {
            return Err(NodeArtifactStoreError::Integrity(
                "artifact blob changed after admission".into(),
            ));
        }
        Ok(())
    }

    async fn write_receipt(
        &self,
        descriptor: &NodeArtifactDescriptor,
    ) -> Result<(), NodeArtifactStoreError> {
        let receipt = ArtifactReceipt {
            schema: RECEIPT_SCHEMA.into(),
            artifact: descriptor.artifact.clone(),
            size_bytes: descriptor.size_bytes,
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not encode artifact receipt: {error}"))
        })?;
        let final_path = self.receipt_path(&descriptor.artifact.digest)?;
        let staging = self
            .root
            .join("staging")
            .join(format!("{}.receipt", Uuid::now_v7()));
        let result = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging)
                .await
                .map_err(|error| {
                    NodeArtifactStoreError::Storage(format!(
                        "could not stage artifact receipt: {error}"
                    ))
                })?;
            file.write_all(&bytes).await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!(
                    "could not write artifact receipt: {error}"
                ))
            })?;
            file.sync_all().await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!("could not sync artifact receipt: {error}"))
            })?;
            drop(file);
            tokio::fs::rename(&staging, &final_path)
                .await
                .map_err(|error| {
                    NodeArtifactStoreError::Storage(format!(
                        "could not commit artifact receipt: {error}"
                    ))
                })
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&staging).await;
        }
        result
    }

    async fn repair_receipt_if_possible(
        &self,
        descriptor: &NodeArtifactDescriptor,
    ) -> Result<bool, NodeArtifactStoreError> {
        let blob = self.blob_path(&descriptor.artifact.digest)?;
        let receipt = self.receipt_path(&descriptor.artifact.digest)?;
        let blob_exists = tokio::fs::try_exists(&blob).await.map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not inspect artifact blob: {error}"))
        })?;
        let receipt_exists = tokio::fs::try_exists(&receipt).await.map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not inspect artifact receipt: {error}"))
        })?;
        if !blob_exists || receipt_exists {
            return Ok(false);
        }
        let mut file = tokio::fs::File::open(&blob).await.map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not open orphan artifact blob: {error}"))
        })?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!(
                    "could not verify orphan artifact: {error}"
                ))
            })?;
            if read == 0 {
                break;
            }
            size = size.saturating_add(read as u64);
            digest.update(&buffer[..read]);
        }
        if size != descriptor.size_bytes
            || format!("sha256:{:x}", digest.finalize()) != descriptor.artifact.digest
        {
            return Err(NodeArtifactStoreError::Integrity(
                "orphan artifact blob does not match the requested identity".into(),
            ));
        }
        self.write_receipt(descriptor).await?;
        Ok(true)
    }
}

#[async_trait]
impl INodeArtifactStore for LocalNodeArtifactStore {
    async fn put(
        &self,
        descriptor: &NodeArtifactDescriptor,
        reader: NodeArtifactReader,
    ) -> Result<NodeArtifactWrite, NodeArtifactStoreError> {
        descriptor
            .validate()
            .map_err(NodeArtifactStoreError::Invalid)?;
        if descriptor.size_bytes > self.maximum_blob_bytes {
            return Err(NodeArtifactStoreError::Invalid(
                "artifact exceeds the configured blob limit".into(),
            ));
        }
        self.ensure_directories().await?;
        let staging = self.stage(descriptor, reader).await?;
        let lock = self.lock().await?;
        let result = async {
            if self.repair_receipt_if_possible(descriptor).await? {
                return Ok(NodeArtifactWrite {
                    descriptor: descriptor.clone(),
                    replayed: true,
                });
            }
            if let Some(existing) = self.stored_descriptor(&descriptor.artifact).await? {
                if existing != *descriptor {
                    return Err(NodeArtifactStoreError::Conflict);
                }
                return Ok(NodeArtifactWrite {
                    descriptor: existing,
                    replayed: true,
                });
            }
            let blob = self.blob_path(&descriptor.artifact.digest)?;
            tokio::fs::rename(&staging, &blob).await.map_err(|error| {
                NodeArtifactStoreError::Storage(format!("could not commit artifact blob: {error}"))
            })?;
            self.write_receipt(descriptor).await?;
            Ok(NodeArtifactWrite {
                descriptor: descriptor.clone(),
                replayed: false,
            })
        }
        .await;
        drop(lock);
        let _ = tokio::fs::remove_file(&staging).await;
        result
    }

    async fn open(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<OpenNodeArtifact, NodeArtifactStoreError> {
        validate_cloud_artifact(artifact).map_err(NodeArtifactStoreError::Invalid)?;
        self.ensure_directories().await?;
        let descriptor = self
            .stored_descriptor(artifact)
            .await?
            .ok_or(NodeArtifactStoreError::NotFound)?;
        let path = self.blob_path(&artifact.digest)?;
        let reader = tokio::fs::File::open(path).await.map_err(|error| {
            NodeArtifactStoreError::Storage(format!("could not open artifact blob: {error}"))
        })?;
        Ok(OpenNodeArtifact {
            descriptor,
            reader: Box::pin(reader),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
    use tokio::io::AsyncReadExt;

    fn descriptor(bytes: &[u8]) -> NodeArtifactDescriptor {
        let digest = format!("sha256:{:x}", Sha256::digest(bytes));
        NodeArtifactDescriptor::new(
            ArtifactRef {
                uri: artifact_uri(&digest).expect("artifact URI"),
                digest,
                media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
            },
            bytes.len() as u64,
        )
        .expect("descriptor")
    }

    fn reader(bytes: &[u8]) -> NodeArtifactReader {
        Box::pin(std::io::Cursor::new(bytes.to_vec()))
    }

    #[tokio::test]
    async fn content_addressed_write_replays_and_streams_exact_bytes() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"durable artifact bytes";
        let descriptor = descriptor(bytes);

        let first = store
            .put(&descriptor, reader(bytes))
            .await
            .expect("first write");
        assert!(!first.replayed);
        let replay = store
            .put(&descriptor, reader(bytes))
            .await
            .expect("replayed write");
        assert!(replay.replayed);

        let mut opened = store
            .open(&descriptor.artifact)
            .await
            .expect("open artifact");
        let mut actual = Vec::new();
        opened
            .reader
            .read_to_end(&mut actual)
            .await
            .expect("read artifact");
        assert_eq!(actual, bytes);
        assert_eq!(opened.descriptor, descriptor);
    }

    #[tokio::test]
    async fn digest_mismatch_and_media_type_conflict_fail_closed() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"artifact";
        let descriptor = descriptor(bytes);
        assert!(matches!(
            store.put(&descriptor, reader(b"tampered")).await,
            Err(NodeArtifactStoreError::Integrity(_))
        ));

        store
            .put(&descriptor, reader(bytes))
            .await
            .expect("stored artifact");
        let mut forged = descriptor.artifact.clone();
        forged.media_type = "application/octet-stream".into();
        assert!(matches!(
            store.open(&forged).await,
            Err(NodeArtifactStoreError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn blob_commit_gap_is_repaired_idempotently() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"crash-gap artifact";
        let descriptor = descriptor(bytes);
        store.ensure_directories().await.expect("directories");
        let blob = store
            .blob_path(&descriptor.artifact.digest)
            .expect("blob path");
        tokio::fs::write(blob, bytes).await.expect("orphan blob");

        let replay = store
            .put(&descriptor, reader(bytes))
            .await
            .expect("repair write");
        assert!(replay.replayed);
        store
            .open(&descriptor.artifact)
            .await
            .expect("repaired artifact");
    }

    #[tokio::test]
    async fn same_length_blob_tampering_is_rejected_before_download_or_replay() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"trusted artifact";
        let descriptor = descriptor(bytes);
        store
            .put(&descriptor, reader(bytes))
            .await
            .expect("stored artifact");
        let blob = store
            .blob_path(&descriptor.artifact.digest)
            .expect("blob path");
        tokio::fs::write(&blob, b"forged! artifact")
            .await
            .expect("tamper blob");

        assert!(matches!(
            store.open(&descriptor.artifact).await,
            Err(NodeArtifactStoreError::Integrity(_))
        ));
        assert!(matches!(
            store.put(&descriptor, reader(bytes)).await,
            Err(NodeArtifactStoreError::Integrity(_))
        ));
    }
}
