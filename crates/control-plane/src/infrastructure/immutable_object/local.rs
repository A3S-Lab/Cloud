use super::stream::{
    validate_observed_identity, ImmutableObjectOpen, ImmutableObjectOpenResult,
    ImmutableObjectReader,
};
use super::{ImmutableObjectError, ImmutableObjectRead, ImmutableObjectWrite};
use crate::infrastructure::sync_directory;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

const STREAM_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(super) struct LocalBackend {
    root: PathBuf,
}

impl LocalBackend {
    pub(super) fn new(root: PathBuf) -> Result<Self, ImmutableObjectError> {
        if root.as_os_str().is_empty()
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ImmutableObjectError::Invalid(
                "local object root is invalid".into(),
            ));
        }
        create_secure_directory(&root)?;
        create_secure_directory(&root.join(".immutable-object-staging"))?;
        Ok(Self { root })
    }

    pub(super) fn put(
        &self,
        scoped_key: &str,
        conflict_key: &str,
        body: &[u8],
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        self.ensure_parent(scoped_key)?;
        let target = self.root.join(scoped_key);
        match fs::symlink_metadata(&target) {
            Ok(_) => return self.replay(scoped_key, conflict_key, body),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error("inspect immutable object", error)),
        }
        let parent = target.parent().ok_or_else(|| {
            ImmutableObjectError::Invalid("immutable object has no parent".into())
        })?;
        let temporary = self
            .root
            .join(".immutable-object-staging")
            .join(Uuid::now_v7().to_string());
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&temporary)
                .map_err(|error| io_error("create immutable object staging file", error))?;
            file.write_all(body)
                .map_err(|error| io_error("write immutable object staging file", error))?;
            file.sync_all()
                .map_err(|error| io_error("sync immutable object staging file", error))?;
            match fs::hard_link(&temporary, &target) {
                Ok(()) => {
                    sync_directory(parent)
                        .map_err(|error| io_error("sync immutable object directory", error))?;
                    Ok(ImmutableObjectWrite { created: true })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    self.replay(scoped_key, conflict_key, body)
                }
                Err(error) => Err(io_error("publish immutable object", error)),
            }
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    pub(super) fn get(
        &self,
        scoped_key: &str,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectRead, ImmutableObjectError> {
        let path = self.root.join(scoped_key);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ImmutableObjectRead::Missing)
            }
            Err(error) => return Err(io_error("inspect immutable object", error)),
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > maximum_bytes
        {
            return Ok(ImmutableObjectRead::Corrupt);
        }
        let body = fs::read(path).map_err(|error| io_error("read immutable object", error))?;
        if body.len() as u64 > maximum_bytes {
            return Ok(ImmutableObjectRead::Corrupt);
        }
        Ok(ImmutableObjectRead::Found(body))
    }

    pub(super) fn remove(&self, scoped_key: &str) -> Result<(), ImmutableObjectError> {
        match fs::remove_file(self.root.join(scoped_key)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error("remove immutable object", error)),
        }
    }

    pub(super) async fn put_stream(
        &self,
        scoped_key: String,
        object_key: String,
        mut reader: ImmutableObjectReader,
        expected_size: u64,
        expected_digest: String,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        let temporary = self
            .root
            .join(".immutable-object-staging")
            .join(Uuid::now_v7().to_string());
        let mut options = tokio::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options
            .open(&temporary)
            .await
            .map_err(|error| io_error("create immutable object staging file", error))?;
        let staged = async {
            let mut digest = Sha256::new();
            let mut size = 0_u64;
            let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
            loop {
                let read = reader.read(&mut buffer).await.map_err(|error| {
                    ImmutableObjectError::Unavailable(format!(
                        "could not read immutable object upload: {error}"
                    ))
                })?;
                if read == 0 {
                    break;
                }
                size = size.checked_add(read as u64).ok_or_else(|| {
                    ImmutableObjectError::Invalid("immutable object upload size overflowed".into())
                })?;
                if size > expected_size || size > maximum_bytes {
                    return Err(ImmutableObjectError::Invalid(
                        "immutable object upload exceeds its declared or configured size".into(),
                    ));
                }
                digest.update(&buffer[..read]);
                file.write_all(&buffer[..read])
                    .await
                    .map_err(|error| io_error("write immutable object staging file", error))?;
            }
            validate_observed_identity(size, digest, expected_size, &expected_digest)?;
            file.sync_all()
                .await
                .map_err(|error| io_error("sync immutable object staging file", error))?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(error) = staged {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error);
        }

        let backend = self.clone();
        let staged_path = temporary.clone();
        let published = tokio::task::spawn_blocking(move || {
            backend.publish_staged(
                &scoped_key,
                &object_key,
                &staged_path,
                expected_size,
                &expected_digest,
            )
        })
        .await
        .map_err(|error| {
            ImmutableObjectError::Unavailable(format!(
                "local immutable object publisher failed: {error}"
            ))
        })?;
        let _ = tokio::fs::remove_file(temporary).await;
        published
    }

    pub(super) async fn open(
        &self,
        scoped_key: String,
        maximum_bytes: u64,
    ) -> Result<ImmutableObjectOpenResult, ImmutableObjectError> {
        let path = self.root.join(scoped_key);
        let metadata = match tokio::fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ImmutableObjectOpenResult::Missing)
            }
            Err(error) => return Err(io_error("inspect immutable object stream", error)),
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > maximum_bytes
        {
            return Ok(ImmutableObjectOpenResult::Corrupt);
        }
        let reader = tokio::fs::File::open(path)
            .await
            .map_err(|error| io_error("open immutable object stream", error))?;
        Ok(ImmutableObjectOpenResult::Found(ImmutableObjectOpen {
            size_bytes: metadata.len(),
            reader: Box::pin(reader),
        }))
    }

    fn replay(
        &self,
        scoped_key: &str,
        conflict_key: &str,
        expected: &[u8],
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        match self.get(scoped_key, expected.len() as u64)? {
            ImmutableObjectRead::Found(existing) if existing == expected => {
                Ok(ImmutableObjectWrite { created: false })
            }
            _ => Err(ImmutableObjectError::Conflict(conflict_key.into())),
        }
    }

    fn publish_staged(
        &self,
        scoped_key: &str,
        object_key: &str,
        staged: &Path,
        expected_size: u64,
        expected_digest: &str,
    ) -> Result<ImmutableObjectWrite, ImmutableObjectError> {
        self.ensure_parent(scoped_key)?;
        let target = self.root.join(scoped_key);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                self.require_exact_file(&target, object_key, expected_size, expected_digest)?;
                return Ok(ImmutableObjectWrite { created: false });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error("inspect immutable object", error)),
        }
        let parent = target.parent().ok_or_else(|| {
            ImmutableObjectError::Invalid("immutable object has no parent".into())
        })?;
        match fs::hard_link(staged, &target) {
            Ok(()) => {
                sync_directory(parent)
                    .map_err(|error| io_error("sync immutable object directory", error))?;
                Ok(ImmutableObjectWrite { created: true })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.require_exact_file(&target, object_key, expected_size, expected_digest)?;
                Ok(ImmutableObjectWrite { created: false })
            }
            Err(error) => Err(io_error("publish immutable object", error)),
        }
    }

    fn require_exact_file(
        &self,
        path: &Path,
        object_key: &str,
        expected_size: u64,
        expected_digest: &str,
    ) -> Result<(), ImmutableObjectError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| io_error("inspect existing immutable object", error))?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != expected_size
        {
            return Err(stored_integrity_error(object_key));
        }
        let mut file = fs::File::open(path)
            .map_err(|error| io_error("open existing immutable object", error))?;
        let mut digest = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; STREAM_BUFFER_BYTES];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| io_error("verify existing immutable object", error))?;
            if read == 0 {
                break;
            }
            size = size.checked_add(read as u64).ok_or_else(|| {
                ImmutableObjectError::Integrity("stored immutable object size overflowed".into())
            })?;
            if size > expected_size {
                return Err(stored_integrity_error(object_key));
            }
            digest.update(&buffer[..read]);
        }
        if size != expected_size || format!("sha256:{:x}", digest.finalize()) != expected_digest {
            return Err(stored_integrity_error(object_key));
        }
        Ok(())
    }

    fn ensure_parent(&self, scoped_key: &str) -> Result<(), ImmutableObjectError> {
        let parent = Path::new(scoped_key).parent().ok_or_else(|| {
            ImmutableObjectError::Invalid("immutable object has no parent".into())
        })?;
        let mut current = self.root.clone();
        for component in parent.components() {
            let Component::Normal(component) = component else {
                return Err(ImmutableObjectError::Invalid(
                    "immutable object path is invalid".into(),
                ));
            };
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Ok(_) => return Err(not_real_directory(&current)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => secure_directory(&current)?,
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                                io_error("inspect immutable object directory", error)
                            })?;
                            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                                return Err(not_real_directory(&current));
                            }
                        }
                        Err(error) => {
                            return Err(io_error("create immutable object directory", error))
                        }
                    }
                }
                Err(error) => return Err(io_error("inspect immutable object directory", error)),
            }
        }
        Ok(())
    }
}

fn create_secure_directory(path: &Path) -> Result<(), ImmutableObjectError> {
    fs::create_dir_all(path)
        .map_err(|error| io_error("create immutable object directory", error))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect immutable object directory", error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ImmutableObjectError::Invalid(format!(
            "immutable object directory {} is not a real directory",
            path.display()
        )));
    }
    secure_directory(path)
}

fn secure_directory(_path: &Path) -> Result<(), ImmutableObjectError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error("secure immutable object directory", error))?;
    }
    Ok(())
}

fn not_real_directory(path: &Path) -> ImmutableObjectError {
    ImmutableObjectError::Unavailable(format!(
        "immutable object directory {} is not a real directory",
        path.display()
    ))
}

fn io_error(action: &str, error: std::io::Error) -> ImmutableObjectError {
    ImmutableObjectError::Unavailable(format!("{action}: {error}"))
}

fn stored_integrity_error(object_key: &str) -> ImmutableObjectError {
    ImmutableObjectError::Integrity(format!(
        "stored immutable object {object_key} does not match its identity"
    ))
}
