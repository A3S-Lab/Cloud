use super::NodeArtifactManager;
use a3s_runtime::{RuntimeError, RuntimeResult};
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(target_os = "linux")]
use super::NodeArtifactError;
#[cfg(target_os = "linux")]
use a3s_box_runtime::{BoxArtifactPort, BoxArtifactPortError};
#[cfg(target_os = "linux")]
use a3s_runtime::contract::{
    RuntimeMount, RuntimeOutputArtifact, RuntimeOutputSpec, RuntimeUnitSpec,
};
#[cfg(target_os = "linux")]
use async_trait::async_trait;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

/// The sole adapter from Box's local storage lifecycle to the node agent's
/// existing authenticated Artifact manager.
///
/// The adapter is installed before enrollment so Runtime capabilities remain
/// stable. Exactly one enrolled manager is bound before command execution;
/// Box continues to own Volume lifecycle while Cloud owns Artifact admission,
/// transport, durable receipts, and publication.
pub(crate) struct CloudBoxArtifactPort {
    manager: RwLock<Option<Arc<NodeArtifactManager>>>,
}

impl CloudBoxArtifactPort {
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn new() -> Self {
        Self {
            manager: RwLock::new(None),
        }
    }

    pub(crate) async fn bind_manager(
        &self,
        manager: Arc<NodeArtifactManager>,
    ) -> RuntimeResult<()> {
        let mut current = self.manager.write().await;
        match current.as_ref() {
            Some(existing) if Arc::ptr_eq(existing, &manager) => Ok(()),
            Some(_) => Err(RuntimeError::RequestConflict {
                request_id: "box-artifact-manager-binding".into(),
            }),
            None => {
                *current = Some(manager);
                Ok(())
            }
        }
    }

    #[cfg(target_os = "linux")]
    async fn manager(&self) -> Result<Arc<NodeArtifactManager>, BoxArtifactPortError> {
        self.manager.read().await.clone().ok_or_else(|| {
            BoxArtifactPortError::Unavailable("Cloud node Artifact manager is not bound".into())
        })
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl BoxArtifactPort for CloudBoxArtifactPort {
    async fn mount_path(
        &self,
        spec: &RuntimeUnitSpec,
        mount: &RuntimeMount,
    ) -> Result<PathBuf, BoxArtifactPortError> {
        self.manager()
            .await?
            .mount_path(spec, mount)
            .await
            .map_err(map_artifact_error)
    }

    async fn capture_output(
        &self,
        spec: &RuntimeUnitSpec,
        output: &RuntimeOutputSpec,
        source: &Path,
    ) -> Result<RuntimeOutputArtifact, BoxArtifactPortError> {
        self.manager()
            .await?
            .capture_output_directory(spec, output, source)
            .await
            .map_err(map_artifact_error)
    }

    async fn cleanup_spec(&self, spec_digest: &str) -> Result<(), BoxArtifactPortError> {
        self.manager()
            .await?
            .cleanup_spec(spec_digest)
            .await
            .map_err(map_artifact_error)
    }
}

#[cfg(target_os = "linux")]
fn map_artifact_error(error: NodeArtifactError) -> BoxArtifactPortError {
    if error.retryable() {
        BoxArtifactPortError::Unavailable(
            "Cloud node Artifact storage is temporarily unavailable".into(),
        )
    } else {
        BoxArtifactPortError::Rejected("Cloud node Artifact request was rejected".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactConfig, DownloadedNodeArtifact, NodeArtifactTransport, NodeControlClientError,
    };
    use a3s_cloud_contracts::{
        NodeArtifactDownloadRequest, NodeArtifactUploadReceipt, NodeArtifactUploadRequest,
    };
    use async_trait::async_trait;
    use std::path::Path;

    struct UnusedTransport;

    #[async_trait]
    impl NodeArtifactTransport for UnusedTransport {
        async fn download(
            &self,
            _request: &NodeArtifactDownloadRequest,
            _destination: &Path,
            _maximum_bytes: u64,
        ) -> Result<DownloadedNodeArtifact, NodeControlClientError> {
            Err(NodeControlClientError::Invalid(
                "binding fixture does not transfer Artifacts".into(),
            ))
        }

        async fn upload(
            &self,
            _request: &NodeArtifactUploadRequest,
            _source: &Path,
        ) -> Result<NodeArtifactUploadReceipt, NodeControlClientError> {
            Err(NodeControlClientError::Invalid(
                "binding fixture does not transfer Artifacts".into(),
            ))
        }
    }

    #[tokio::test]
    async fn box_artifact_port_fences_one_enrolled_manager() {
        let state = tempfile::tempdir().expect("node Artifact state");
        let config = ArtifactConfig {
            max_blob_bytes: 1024 * 1024,
            max_entries: 100,
            max_file_bytes: 512 * 1024,
            max_expanded_bytes: 2 * 1024 * 1024,
        };
        let first = Arc::new(
            NodeArtifactManager::new(
                state.path().join("first"),
                config.clone(),
                uuid::Uuid::now_v7(),
                Arc::new(UnusedTransport),
            )
            .expect("first Artifact manager"),
        );
        let second = Arc::new(
            NodeArtifactManager::new(
                state.path().join("second"),
                config,
                uuid::Uuid::now_v7(),
                Arc::new(UnusedTransport),
            )
            .expect("second Artifact manager"),
        );
        let port = CloudBoxArtifactPort::new();

        port.bind_manager(first.clone())
            .await
            .expect("first manager binding");
        port.bind_manager(first)
            .await
            .expect("idempotent manager binding");
        assert!(matches!(
            port.bind_manager(second).await,
            Err(RuntimeError::RequestConflict { .. })
        ));
    }
}
