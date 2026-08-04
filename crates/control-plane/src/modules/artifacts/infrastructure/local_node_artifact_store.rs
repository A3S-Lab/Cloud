use crate::infrastructure::{
    ImmutableObjectClient, ImmutableObjectError, ImmutableObjectOpenResult, ImmutableObjectRead,
    ImmutableObjectVerification,
};
use crate::modules::artifacts::domain::{
    INodeArtifactStore, NodeArtifactDescriptor, NodeArtifactReader, NodeArtifactStoreError,
    NodeArtifactWrite, OpenNodeArtifact,
};
use a3s_cloud_contracts::validate_cloud_artifact;
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};

const RECEIPT_SCHEMA: &str = "a3s.cloud.node-artifact-object.v1";
const MAX_RECEIPT_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone)]
pub struct LocalNodeArtifactStore {
    blobs: ImmutableObjectClient,
    receipts: ImmutableObjectClient,
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
        let blobs = ImmutableObjectClient::local(root.clone(), "blobs/sha256")
            .map_err(|error| error.to_string())?;
        let receipts = ImmutableObjectClient::local(root, "receipts/sha256")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            blobs,
            receipts,
            maximum_blob_bytes,
        })
    }

    fn digest_hex<'a>(&self, digest: &'a str) -> Result<&'a str, NodeArtifactStoreError> {
        digest.strip_prefix("sha256:").ok_or_else(|| {
            NodeArtifactStoreError::Invalid("artifact digest must use sha256".into())
        })
    }

    fn blob_key<'a>(&self, digest: &'a str) -> Result<&'a str, NodeArtifactStoreError> {
        self.digest_hex(digest)
    }

    fn receipt_key(&self, digest: &str) -> Result<String, NodeArtifactStoreError> {
        Ok(format!("{}.json", self.digest_hex(digest)?))
    }

    async fn read_receipt(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<Option<NodeArtifactDescriptor>, NodeArtifactStoreError> {
        let key = self.receipt_key(&artifact.digest)?;
        let bytes = match self
            .receipts
            .get(&key, MAX_RECEIPT_BYTES)
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectRead::Found(bytes) => bytes,
            ImmutableObjectRead::Missing => return Ok(None),
            ImmutableObjectRead::Corrupt => {
                return Err(NodeArtifactStoreError::Integrity(
                    "artifact receipt exceeds its storage bound".into(),
                ))
            }
        };
        let receipt = serde_json::from_slice::<ArtifactReceipt>(&bytes)
            .map_err(|_| NodeArtifactStoreError::Integrity("artifact receipt is invalid".into()))?;
        if receipt.schema != RECEIPT_SCHEMA
            || receipt.artifact != *artifact
            || receipt.size_bytes == 0
            || receipt.size_bytes > self.maximum_blob_bytes
        {
            return Err(NodeArtifactStoreError::Integrity(
                "artifact receipt does not match its identity".into(),
            ));
        }
        NodeArtifactDescriptor::new(receipt.artifact, receipt.size_bytes)
            .map(Some)
            .map_err(NodeArtifactStoreError::Integrity)
    }

    async fn stored_descriptor(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<Option<NodeArtifactDescriptor>, NodeArtifactStoreError> {
        let Some(descriptor) = self.read_receipt(artifact).await? else {
            return match self
                .blobs
                .open(self.blob_key(&artifact.digest)?, self.maximum_blob_bytes)
                .await
                .map_err(map_object_error)?
            {
                ImmutableObjectOpenResult::Missing => Ok(None),
                ImmutableObjectOpenResult::Found(_) | ImmutableObjectOpenResult::Corrupt => {
                    Err(NodeArtifactStoreError::Integrity(
                        "artifact blob and receipt are incomplete".into(),
                    ))
                }
            };
        };
        match self
            .blobs
            .verify(
                self.blob_key(&artifact.digest)?,
                descriptor.size_bytes,
                &descriptor.artifact.digest,
                self.maximum_blob_bytes,
            )
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectVerification::Verified => Ok(Some(descriptor)),
            ImmutableObjectVerification::Missing | ImmutableObjectVerification::Corrupt => {
                Err(NodeArtifactStoreError::Integrity(
                    "artifact receipt does not match its blob".into(),
                ))
            }
        }
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
        self.receipts
            .put(
                &self.receipt_key(&descriptor.artifact.digest)?,
                bytes,
                MAX_RECEIPT_BYTES,
            )
            .await
            .map(|_| ())
            .map_err(map_object_error)
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

        if let Some(existing) = self.read_receipt(&descriptor.artifact).await? {
            if existing != *descriptor {
                return Err(NodeArtifactStoreError::Conflict);
            }
            match self
                .blobs
                .verify(
                    self.blob_key(&descriptor.artifact.digest)?,
                    descriptor.size_bytes,
                    &descriptor.artifact.digest,
                    self.maximum_blob_bytes,
                )
                .await
                .map_err(map_object_error)?
            {
                ImmutableObjectVerification::Verified => {}
                ImmutableObjectVerification::Missing | ImmutableObjectVerification::Corrupt => {
                    return Err(NodeArtifactStoreError::Integrity(
                        "artifact receipt does not match its blob".into(),
                    ))
                }
            }
        }

        let write = self
            .blobs
            .put_stream(
                self.blob_key(&descriptor.artifact.digest)?,
                reader,
                descriptor.size_bytes,
                &descriptor.artifact.digest,
                self.maximum_blob_bytes,
            )
            .await
            .map_err(map_object_error)?;
        self.write_receipt(descriptor).await?;
        Ok(NodeArtifactWrite {
            descriptor: descriptor.clone(),
            replayed: !write.created,
        })
    }

    async fn open(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<OpenNodeArtifact, NodeArtifactStoreError> {
        validate_cloud_artifact(artifact).map_err(NodeArtifactStoreError::Invalid)?;
        let descriptor = self
            .stored_descriptor(artifact)
            .await?
            .ok_or(NodeArtifactStoreError::NotFound)?;
        match self
            .blobs
            .open(self.blob_key(&artifact.digest)?, self.maximum_blob_bytes)
            .await
            .map_err(map_object_error)?
        {
            ImmutableObjectOpenResult::Found(opened)
                if opened.size_bytes == descriptor.size_bytes =>
            {
                Ok(OpenNodeArtifact {
                    descriptor,
                    reader: opened.reader,
                })
            }
            ImmutableObjectOpenResult::Missing | ImmutableObjectOpenResult::Corrupt => {
                Err(NodeArtifactStoreError::Integrity(
                    "artifact blob changed after verification".into(),
                ))
            }
            ImmutableObjectOpenResult::Found(_) => Err(NodeArtifactStoreError::Integrity(
                "artifact blob size changed after verification".into(),
            )),
        }
    }
}

fn map_object_error(error: ImmutableObjectError) -> NodeArtifactStoreError {
    match error {
        ImmutableObjectError::Invalid(message) => NodeArtifactStoreError::Invalid(message),
        ImmutableObjectError::Conflict(_) => NodeArtifactStoreError::Conflict,
        ImmutableObjectError::Integrity(message) => NodeArtifactStoreError::Integrity(message),
        ImmutableObjectError::Unavailable(message) => NodeArtifactStoreError::Storage(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{
        artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE, SKILL_BUNDLE_MEDIA_TYPE,
    };
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use std::path::Path;
    use tokio::io::AsyncReadExt;

    fn descriptor(bytes: &[u8]) -> NodeArtifactDescriptor {
        descriptor_with_media_type(bytes, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE)
    }

    fn descriptor_with_media_type(bytes: &[u8], media_type: &str) -> NodeArtifactDescriptor {
        let digest = format!("sha256:{:x}", Sha256::digest(bytes));
        NodeArtifactDescriptor::new(
            ArtifactRef {
                uri: artifact_uri(&digest).expect("artifact URI"),
                digest,
                media_type: media_type.into(),
            },
            bytes.len() as u64,
        )
        .expect("descriptor")
    }

    fn reader(bytes: &[u8]) -> NodeArtifactReader {
        Box::pin(Cursor::new(bytes.to_vec()))
    }

    fn blob_path(root: &Path, descriptor: &NodeArtifactDescriptor) -> PathBuf {
        root.join("blobs")
            .join("sha256")
            .join(descriptor.artifact.digest.trim_start_matches("sha256:"))
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
    async fn skill_bundle_write_replays_and_streams_exact_bytes() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"deterministic Skill release tar";
        let descriptor = descriptor_with_media_type(bytes, SKILL_BUNDLE_MEDIA_TYPE);

        let first = store
            .put(&descriptor, reader(bytes))
            .await
            .expect("first Skill bundle write");
        assert!(!first.replayed);
        let replay = store
            .put(&descriptor, reader(bytes))
            .await
            .expect("replayed Skill bundle write");
        assert!(replay.replayed);

        let mut opened = store
            .open(&descriptor.artifact)
            .await
            .expect("open Skill bundle");
        let mut actual = Vec::new();
        opened
            .reader
            .read_to_end(&mut actual)
            .await
            .expect("read Skill bundle");
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
        let blob = blob_path(directory.path(), &descriptor);
        tokio::fs::create_dir_all(blob.parent().expect("blob parent"))
            .await
            .expect("blob directory");
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
        tokio::fs::write(
            blob_path(directory.path(), &descriptor),
            b"forged! artifact",
        )
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

    #[tokio::test]
    async fn receipt_without_blob_is_not_silently_repaired() {
        let directory = tempfile::tempdir().expect("artifact directory");
        let store = LocalNodeArtifactStore::new(directory.path(), 1024).expect("store");
        let bytes = b"receipt-only artifact";
        let descriptor = descriptor(bytes);
        store
            .put(&descriptor, reader(bytes))
            .await
            .expect("stored artifact");
        tokio::fs::remove_file(blob_path(directory.path(), &descriptor))
            .await
            .expect("remove blob");

        assert!(matches!(
            store.put(&descriptor, reader(bytes)).await,
            Err(NodeArtifactStoreError::Integrity(_))
        ));
    }
}
