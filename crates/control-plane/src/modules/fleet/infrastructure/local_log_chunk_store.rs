use crate::modules::fleet::domain::services::{
    ILogChunkStore, LogChunkStoreError, RetrievedLogChunk, StoredLogChunk,
};
use a3s_cloud_contracts::NodeLogChunkReport;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

// A valid one MiB text chunk can expand to six bytes per byte when JSON escaped.
const MAX_LOG_OBJECT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct LocalLogChunkStore {
    root: PathBuf,
}

impl LocalLogChunkStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, LogChunkStoreError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(unavailable("create log object directory"))?;
        secure_directory(&root)?;
        fs::create_dir_all(root.join(".tmp"))
            .map_err(unavailable("create log object temporary directory"))?;
        secure_directory(&root.join(".tmp"))?;
        Ok(Self { root })
    }

    fn object_key(node_id: Uuid, report: &NodeLogChunkReport) -> String {
        let unit_digest = format!("{:x}", Sha256::digest(report.unit_id.as_bytes()));
        let cursor_digest = format!("{:x}", Sha256::digest(report.chunk.cursor.as_bytes()));
        format!(
            "nodes/{node_id}/units/{unit_digest}/generations/{}/chunks/{:020}-{cursor_digest}.json",
            report.generation, report.chunk.sequence
        )
    }
}

#[async_trait]
impl ILogChunkStore for LocalLogChunkStore {
    async fn put(
        &self,
        _batch_id: Uuid,
        node_id: Uuid,
        _ordinal: u16,
        report: &NodeLogChunkReport,
    ) -> Result<StoredLogChunk, LogChunkStoreError> {
        report.validate().map_err(LogChunkStoreError::Invalid)?;
        if node_id.is_nil() {
            return Err(LogChunkStoreError::Invalid(
                "node ID must not be nil".into(),
            ));
        }
        let object_key = Self::object_key(node_id, report);
        let body = serde_json::to_vec(report)
            .map_err(|error| LogChunkStoreError::Invalid(error.to_string()))?;
        let root = self.root.clone();
        let write_key = object_key.clone();
        let created = tokio::task::spawn_blocking(move || write_once(&root, &write_key, &body))
            .await
            .map_err(|error| {
                LogChunkStoreError::Unavailable(format!("log object writer failed: {error}"))
            })??;
        Ok(StoredLogChunk {
            object_key,
            created,
        })
    }

    async fn get(
        &self,
        object_key: &str,
        expected_checksum: &str,
    ) -> Result<RetrievedLogChunk, LogChunkStoreError> {
        validate_object_key(object_key)?;
        if !is_sha256(expected_checksum) {
            return Err(LogChunkStoreError::Invalid(
                "expected log checksum is invalid".into(),
            ));
        }
        let root = self.root.clone();
        let object_key = object_key.to_owned();
        let expected_checksum = expected_checksum.to_owned();
        tokio::task::spawn_blocking(move || read_verified(&root, &object_key, &expected_checksum))
            .await
            .map_err(|error| {
                LogChunkStoreError::Unavailable(format!("log object reader failed: {error}"))
            })?
    }

    async fn remove(&self, object_key: &str) -> Result<(), LogChunkStoreError> {
        validate_object_key(object_key)?;
        let root = self.root.clone();
        let object_key = object_key.to_owned();
        tokio::task::spawn_blocking(move || {
            let path = root.join(object_key);
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(unavailable("remove log object")(error)),
            }
        })
        .await
        .map_err(|error| {
            LogChunkStoreError::Unavailable(format!("log object remover failed: {error}"))
        })?
    }

    async fn health(&self) -> Result<bool, LogChunkStoreError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let probe = root.join(".tmp").join(format!("health-{}", Uuid::now_v7()));
            let result = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&probe)
                .and_then(|mut file| {
                    file.write_all(b"ok")?;
                    file.sync_all()
                });
            let _ = fs::remove_file(&probe);
            match result {
                Ok(()) => Ok(true),
                Err(error) => Err(unavailable("write log store health probe")(error)),
            }
        })
        .await
        .map_err(|error| {
            LogChunkStoreError::Unavailable(format!("log store health check failed: {error}"))
        })?
    }
}

fn read_verified(
    root: &Path,
    object_key: &str,
    expected_checksum: &str,
) -> Result<RetrievedLogChunk, LogChunkStoreError> {
    validate_object_key(object_key)?;
    let path = root.join(object_key);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RetrievedLogChunk::Missing)
        }
        Err(error) => return Err(unavailable("inspect log object")(error)),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_LOG_OBJECT_BYTES
    {
        return Ok(RetrievedLogChunk::Corrupt);
    }
    let body = fs::read(path).map_err(unavailable("read log object"))?;
    let report = match serde_json::from_slice::<NodeLogChunkReport>(&body) {
        Ok(report) => report,
        Err(_) => return Ok(RetrievedLogChunk::Corrupt),
    };
    if report.validate().is_err() || report.checksum != expected_checksum {
        return Ok(RetrievedLogChunk::Corrupt);
    }
    Ok(RetrievedLogChunk::Found(report))
}

fn write_once(root: &Path, object_key: &str, body: &[u8]) -> Result<bool, LogChunkStoreError> {
    validate_object_key(object_key)?;
    let target = root.join(object_key);
    if target.exists() {
        return compare_existing(&target, body).map(|()| false);
    }
    let parent = target
        .parent()
        .ok_or_else(|| LogChunkStoreError::Invalid("log object has no parent".into()))?;
    fs::create_dir_all(parent).map_err(unavailable("create log object parent"))?;
    secure_directory(parent)?;
    let temporary = root.join(".tmp").join(Uuid::now_v7().to_string());
    let write_result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(unavailable("create temporary log object"))?;
        file.write_all(body)
            .map_err(unavailable("write temporary log object"))?;
        file.sync_all()
            .map_err(unavailable("sync temporary log object"))?;
        match fs::hard_link(&temporary, &target) {
            Ok(()) => {
                sync_directory(parent)?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                compare_existing(&target, body)?;
                Ok(false)
            }
            Err(error) => Err(unavailable("publish immutable log object")(error)),
        }
    })();
    let _ = fs::remove_file(&temporary);
    write_result
}

fn compare_existing(path: &Path, expected: &[u8]) -> Result<(), LogChunkStoreError> {
    let existing = fs::read(path).map_err(unavailable("read existing log object"))?;
    if existing == expected {
        Ok(())
    } else {
        Err(LogChunkStoreError::Conflict(path.display().to_string()))
    }
}

fn validate_object_key(object_key: &str) -> Result<(), LogChunkStoreError> {
    let path = Path::new(object_key);
    if object_key.is_empty()
        || object_key.len() > 4096
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LogChunkStoreError::Invalid(
            "log object key is invalid".into(),
        ));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn secure_directory(path: &Path) -> Result<(), LogChunkStoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(unavailable("secure log object directory"))?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), LogChunkStoreError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(unavailable("sync log object directory"))
}

fn unavailable(action: &'static str) -> impl FnOnce(std::io::Error) -> LogChunkStoreError {
    move |error| LogChunkStoreError::Unavailable(format!("{action}: {error}"))
}
