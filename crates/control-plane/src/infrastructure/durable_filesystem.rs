use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Flushes directory metadata through the platform's native directory handle.
///
/// Callers must flush file contents before invoking this function. The
/// directory flush then makes create, rename, and removal metadata durable.
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let directory = {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;

        // Opening a directory on Windows requires backup semantics. Write
        // access is required for FlushFileBuffers; read-only handles return
        // ERROR_ACCESS_DENIED on supported Windows filesystems.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?
    };
    #[cfg(not(windows))]
    let directory = fs::File::open(path)?;

    directory.sync_all()
}

/// Flushes a file created by an external process after reopening it.
///
/// Windows requires a write-capable handle for `FlushFileBuffers`; a
/// read-only handle is sufficient on Unix. Keeping that distinction here
/// prevents individual stores from weakening or reimplementing durability.
pub(crate) fn sync_file(path: &Path) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.write(true);
    options.open(path)?.sync_all()
}

/// Runs one or more blocking directory flushes outside the async executor.
pub(crate) async fn sync_directories(paths: Vec<PathBuf>) -> io::Result<()> {
    tokio::task::spawn_blocking(move || {
        for path in paths {
            sync_directory(&path)?;
        }
        Ok(())
    })
    .await
    .map_err(|error| io::Error::other(format!("directory sync task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn syncs_directory_metadata_through_the_shared_platform_primitive() {
        let root = tempfile::tempdir().expect("temporary durable directory");
        std::fs::write(root.path().join("object"), b"durable").expect("write object");

        sync_directory(root.path()).expect("synchronous directory flush");
        sync_file(&root.path().join("object")).expect("file flush");
        sync_directories(vec![root.path().to_owned()])
            .await
            .expect("asynchronous directory flush");
    }
}
