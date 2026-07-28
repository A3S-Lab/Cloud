use super::{ImmutableObjectError, ImmutableObjectRead, ImmutableObjectWrite};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

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
                    sync_directory(parent)?;
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

fn secure_directory(path: &Path) -> Result<(), ImmutableObjectError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| io_error("secure immutable object directory", error))?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), ImmutableObjectError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync immutable object directory", error))
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
