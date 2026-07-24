use crate::modules::artifacts::domain::{INodeArtifactStore, NodeArtifactDescriptor};
use a3s_cloud_contracts::{
    artifact_uri, NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
};
use a3s_cloud_node_agent::{DownloadedNodeArtifact, NodeArtifactTransport, NodeControlClientError};
use a3s_runtime::contract::{ArtifactRef, RuntimeOutputArtifact};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub(super) struct LocalArtifactTransport {
    artifacts: Arc<dyn INodeArtifactStore>,
}

impl LocalArtifactTransport {
    pub(super) fn new(artifacts: Arc<dyn INodeArtifactStore>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl NodeArtifactTransport for LocalArtifactTransport {
    async fn download(
        &self,
        request: &NodeArtifactDownloadRequest,
        destination: &Path,
        maximum_bytes: u64,
    ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let artifact = request
            .artifact()
            .map_err(NodeControlClientError::Invalid)?;
        let mut opened = self
            .artifacts
            .open(&artifact)
            .await
            .map_err(transport_error)?;
        if opened.descriptor.size_bytes > maximum_bytes {
            return Err(NodeControlClientError::Invalid(
                "Runtime BuildKit input exceeds the node transfer bound".into(),
            ));
        }
        let mut destination = tokio::fs::File::create(destination)
            .await
            .map_err(io_transport_error)?;
        let copied = tokio::io::copy(&mut opened.reader, &mut destination)
            .await
            .map_err(io_transport_error)?;
        destination.flush().await.map_err(io_transport_error)?;
        if copied != opened.descriptor.size_bytes {
            return Err(NodeControlClientError::Invalid(
                "Runtime BuildKit input changed during transfer".into(),
            ));
        }
        Ok(DownloadedNodeArtifact { size_bytes: copied })
    }

    async fn upload(
        &self,
        request: &NodeArtifactUploadRequest,
        source: &Path,
    ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
        request
            .validate()
            .map_err(NodeControlClientError::Invalid)?;
        let artifact = ArtifactRef {
            uri: artifact_uri(&request.digest).map_err(NodeControlClientError::Invalid)?,
            digest: request.digest.clone(),
            media_type: request.media_type.clone(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, request.size_bytes)
            .map_err(NodeControlClientError::Invalid)?;
        let file = tokio::fs::File::open(source)
            .await
            .map_err(io_transport_error)?;
        let stored = self
            .artifacts
            .put(&descriptor, Box::pin(file))
            .await
            .map_err(transport_error)?;
        Ok(NodeArtifactUploadReceipt {
            schema: NodeArtifactUploadReceipt::SCHEMA.into(),
            node_id: request.node_id,
            command_id: request.command_id,
            spec_digest: request.spec_digest.clone(),
            artifact: RuntimeOutputArtifact {
                name: request.output_name.clone(),
                artifact: stored.descriptor.artifact,
                size_bytes: stored.descriptor.size_bytes,
            },
            replayed: stored.replayed,
        })
    }
}

fn transport_error(error: impl std::fmt::Display) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}

fn io_transport_error(error: std::io::Error) -> NodeControlClientError {
    NodeControlClientError::Transport(error.to_string())
}
