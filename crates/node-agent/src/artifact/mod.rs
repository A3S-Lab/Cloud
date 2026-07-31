use crate::NodeControlClientError;
use a3s_cloud_contracts::{
    NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
};
use async_trait::async_trait;
use std::path::Path;

mod archive;
mod box_port;
mod cache_gc;
mod cache_io;
#[cfg(any(target_os = "linux", test))]
mod directory_archive;
mod manager;
mod store;

pub(crate) use box_port::CloudBoxArtifactPort;
pub use manager::NodeArtifactManager;
pub(crate) use store::NodeArtifactCache;
pub use store::{LocalArtifactReader, NodeArtifactError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadedNodeArtifact {
    pub size_bytes: u64,
}

#[async_trait]
pub trait NodeArtifactTransport: Send + Sync {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError>;

    async fn upload(
        &self,
        request: &NodeArtifactUploadRequest,
        source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError>;
}

#[cfg(test)]
mod tests;
