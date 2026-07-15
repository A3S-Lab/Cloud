use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub(crate) struct StateLock(File);

impl StateLock {
    pub(crate) fn exclusive(path: &Path) -> Result<Self, SecureStateError> {
        reject_symlink(path, "state lock")?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path).map_err(io_error("open state lock"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(io_error("secure state lock"))?;
        }
        file.lock_exclusive().map_err(io_error("lock state"))?;
        Ok(Self(file))
    }

    pub(crate) fn try_exclusive(path: &Path) -> Result<Self, SecureStateError> {
        reject_symlink(path, "process lock")?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path).map_err(io_error("open process lock"))?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                SecureStateError::Invalid("another node-agent process holds the state lock".into())
            } else {
                io_error("lock node-agent process")(error)
            }
        })?;
        Ok(Self(file))
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub(crate) fn ensure_directory(path: &Path) -> Result<(), SecureStateError> {
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path).map_err(io_error("inspect state path"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(SecureStateError::Invalid(format!(
                "state path {} is not a real directory",
                path.display()
            )));
        }
    } else {
        std::fs::create_dir_all(path).map_err(io_error("create state directory"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(io_error("secure state directory"))?;
    }
    Ok(())
}

pub(crate) fn read_json<T>(path: &Path, label: &str) -> Result<Option<T>, SecureStateError>
where
    T: DeserializeOwned,
{
    reject_symlink(path, label)?;
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(io_error("inspect state record"))?;
    if !metadata.is_file() {
        return Err(SecureStateError::Invalid(format!(
            "{label} is not a regular file"
        )));
    }
    let mut bytes = Vec::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(io_error("read state record"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| SecureStateError::Invalid(format!("{label} is invalid: {error}")))
}

pub(crate) fn atomic_write<T>(path: &Path, value: &T) -> Result<(), SecureStateError>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .ok_or_else(|| SecureStateError::Invalid("state record path has no parent".into()))?;
    let bytes = serde_json::to_vec(value)
        .map_err(|error| SecureStateError::Invalid(format!("encode state record: {error}")))?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(io_error("create state staging file"))?;
    temporary
        .write_all(&bytes)
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(io_error("write state record"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(io_error("secure state record"))?;
    }
    temporary
        .persist(path)
        .map_err(|error| io_error("publish state record")(error.error))?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error("sync state directory"))?;
    Ok(())
}

fn reject_symlink(path: &Path, label: &str) -> Result<(), SecureStateError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SecureStateError::Invalid(
            format!("{label} {} must not be a symbolic link", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("inspect state file")(error)),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SecureStateError {
    #[error("invalid secure state: {0}")]
    Invalid(String),
    #[error("secure state storage failed: {0}")]
    Storage(String),
}

fn io_error(action: &'static str) -> impl FnOnce(std::io::Error) -> SecureStateError {
    move |error| SecureStateError::Storage(format!("could not {action}: {error}"))
}
