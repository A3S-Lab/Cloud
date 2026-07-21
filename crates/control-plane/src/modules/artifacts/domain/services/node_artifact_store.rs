use a3s_cloud_contracts::validate_cloud_artifact;
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use std::pin::Pin;
use tokio::io::AsyncRead;

pub type NodeArtifactReader = Pin<Box<dyn AsyncRead + Send + Unpin + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeArtifactDescriptor {
    pub artifact: ArtifactRef,
    pub size_bytes: u64,
}

impl NodeArtifactDescriptor {
    pub fn new(artifact: ArtifactRef, size_bytes: u64) -> Result<Self, String> {
        let descriptor = Self {
            artifact,
            size_bytes,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_cloud_artifact(&self.artifact)?;
        if self.size_bytes == 0 {
            return Err("node artifact size must be positive".into());
        }
        Ok(())
    }
}

pub struct OpenNodeArtifact {
    pub descriptor: NodeArtifactDescriptor,
    pub reader: NodeArtifactReader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeArtifactWrite {
    pub descriptor: NodeArtifactDescriptor,
    pub replayed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum NodeArtifactStoreError {
    #[error("node artifact request is invalid: {0}")]
    Invalid(String),
    #[error("node artifact was not found")]
    NotFound,
    #[error("node artifact identity conflicts with stored content")]
    Conflict,
    #[error("node artifact failed integrity validation: {0}")]
    Integrity(String),
    #[error("node artifact storage failed: {0}")]
    Storage(String),
}

#[async_trait]
pub trait INodeArtifactStore: Send + Sync {
    async fn put(
        &self,
        descriptor: &NodeArtifactDescriptor,
        reader: NodeArtifactReader,
    ) -> Result<NodeArtifactWrite, NodeArtifactStoreError>;

    async fn open(
        &self,
        artifact: &ArtifactRef,
    ) -> Result<OpenNodeArtifact, NodeArtifactStoreError>;
}
