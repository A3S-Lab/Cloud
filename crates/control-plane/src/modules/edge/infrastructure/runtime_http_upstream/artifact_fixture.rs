use a3s_cloud_contracts::{
    NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
};
use a3s_cloud_node_agent::{
    ArtifactConfig, DownloadedNodeArtifact, NodeArtifactManager, NodeArtifactTransport,
    NodeControlClientError,
};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

struct RejectingArtifactTransport;

#[async_trait]
impl NodeArtifactTransport for RejectingArtifactTransport {
    async fn download(
        &self,
        _request: &NodeArtifactDownloadRequest,
        _destination: &Path,
        _maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "Artifact-free Box health fixture attempted a download".into(),
        ))
    }

    async fn upload(
        &self,
        _request: &NodeArtifactUploadRequest,
        _source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        Err(NodeControlClientError::Invalid(
            "Artifact-free Box health fixture attempted an upload".into(),
        ))
    }
}

pub(super) fn manager(
    state_root: &Path,
    node_id: Uuid,
) -> Result<Arc<NodeArtifactManager>, String> {
    NodeArtifactManager::new(
        state_root,
        ArtifactConfig {
            max_blob_bytes: 1024 * 1024,
            max_entries: 100,
            max_file_bytes: 512 * 1024,
            max_expanded_bytes: 2 * 1024 * 1024,
        },
        node_id,
        Arc::new(RejectingArtifactTransport),
    )
    .map(Arc::new)
}
