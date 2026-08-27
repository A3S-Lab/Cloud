use a3s_cloud_control_plane::modules::agents::{
    AgentExecutionCheckpointObjectError, AgentExecutionCheckpointObjectReference,
    AgentExecutionCheckpointObjectWrite, IAgentExecutionCheckpointObjectStore,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use std::io;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub(super) struct DurableCheckpointObjectStore {
    root: PathBuf,
}

impl DurableCheckpointObjectStore {
    pub(super) fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub(super) fn object_path(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<PathBuf, AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        Ok(self
            .root
            .join(reference.namespace.as_str())
            .join(reference.object_ref.as_str()))
    }

    fn validate_body(
        reference: &AgentExecutionCheckpointObjectReference,
        body: &[u8],
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        reference
            .validate()
            .map_err(AgentExecutionCheckpointObjectError::Invalid)?;
        if u64::try_from(body.len()).map_err(|_| {
            AgentExecutionCheckpointObjectError::Integrity(
                "checkpoint body length overflowed its committed representation".into(),
            )
        })? != reference.size_bytes
            || Sha256Digest::from_bytes(body) != reference.digest
        {
            return Err(AgentExecutionCheckpointObjectError::Integrity(
                "checkpoint bytes changed their committed digest or length".into(),
            ));
        }
        Ok(())
    }

    async fn replay(
        path: &Path,
        reference: &AgentExecutionCheckpointObjectReference,
        expected: &[u8],
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        let existing = tokio::fs::read(path).await.map_err(unavailable)?;
        if existing != expected {
            return Err(AgentExecutionCheckpointObjectError::Conflict(
                reference.object_ref.clone(),
            ));
        }
        Self::validate_body(reference, &existing)?;
        Ok(AgentExecutionCheckpointObjectWrite { replayed: true })
    }
}

#[async_trait]
impl IAgentExecutionCheckpointObjectStore for DurableCheckpointObjectStore {
    async fn put(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
        body: Vec<u8>,
    ) -> Result<AgentExecutionCheckpointObjectWrite, AgentExecutionCheckpointObjectError> {
        Self::validate_body(reference, &body)?;
        let path = self.object_path(reference)?;
        let parent = path.parent().ok_or_else(|| {
            AgentExecutionCheckpointObjectError::Invalid(
                "checkpoint object path has no parent".into(),
            )
        })?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(unavailable)?;
        let file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .await;
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Self::replay(&path, reference, &body).await;
            }
            Err(error) => return Err(unavailable(error)),
        };
        file.write_all(&body).await.map_err(unavailable)?;
        file.sync_all().await.map_err(unavailable)?;
        Ok(AgentExecutionCheckpointObjectWrite { replayed: false })
    }

    async fn get(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<Vec<u8>, AgentExecutionCheckpointObjectError> {
        let path = self.object_path(reference)?;
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(AgentExecutionCheckpointObjectError::NotFound);
            }
            Err(error) => return Err(unavailable(error)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AgentExecutionCheckpointObjectError::Integrity(
                "checkpoint object is not a regular file".into(),
            ));
        }
        let body = tokio::fs::read(path).await.map_err(unavailable)?;
        Self::validate_body(reference, &body)?;
        Ok(body)
    }
}

fn unavailable(error: io::Error) -> AgentExecutionCheckpointObjectError {
    AgentExecutionCheckpointObjectError::Unavailable(error.to_string())
}
