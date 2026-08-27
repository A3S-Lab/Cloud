use a3s_cloud_control_plane::modules::agents::{
    AgentExecutionCheckpointObjectError, AgentExecutionCheckpointObjectInventoryEntry,
    AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectReference,
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

    async fn inventory_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<AgentExecutionCheckpointObjectInventoryPage, AgentExecutionCheckpointObjectError>
    {
        if limit == 0 || limit > 1_000 {
            return Err(AgentExecutionCheckpointObjectError::Invalid(
                "checkpoint recovery inventory limit is invalid".into(),
            ));
        }
        let namespace = self.root.join("agent-checkpoints");
        let mut entries = tokio::task::spawn_blocking(move || inventory(namespace))
            .await
            .map_err(|error| {
                AgentExecutionCheckpointObjectError::Unavailable(format!(
                    "checkpoint recovery inventory task failed: {error}"
                ))
            })?
            .map_err(unavailable)?;
        entries.retain(|entry| after.is_none_or(|after| entry.object_ref.as_str() > after));
        let has_more = entries.len() > limit;
        entries.truncate(limit);
        let next_after = has_more
            .then(|| entries.last().map(|entry| entry.object_ref.clone()))
            .flatten();
        Ok(AgentExecutionCheckpointObjectInventoryPage {
            entries,
            next_after,
        })
    }

    async fn remove(
        &self,
        reference: &AgentExecutionCheckpointObjectReference,
    ) -> Result<(), AgentExecutionCheckpointObjectError> {
        let path = self.object_path(reference)?;
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(unavailable(error)),
        }
    }
}

fn inventory(namespace: PathBuf) -> io::Result<Vec<AgentExecutionCheckpointObjectInventoryEntry>> {
    if !namespace.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![namespace.clone()];
    let mut entries = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "checkpoint recovery inventory encountered a symbolic link",
                ));
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !metadata.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "checkpoint recovery inventory encountered a non-file object",
                ));
            }
            let path = entry.path();
            let relative = path.strip_prefix(&namespace).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "checkpoint recovery inventory escaped its namespace",
                )
            })?;
            let object_ref = relative
                .components()
                .map(|component| component.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "checkpoint recovery inventory path is not UTF-8",
                    )
                })?
                .join("/");
            entries.push(AgentExecutionCheckpointObjectInventoryEntry {
                object_ref,
                size_bytes: metadata.len(),
            });
        }
    }
    entries.sort_by(|left, right| left.object_ref.cmp(&right.object_ref));
    Ok(entries)
}

fn unavailable(error: io::Error) -> AgentExecutionCheckpointObjectError {
    AgentExecutionCheckpointObjectError::Unavailable(error.to_string())
}
