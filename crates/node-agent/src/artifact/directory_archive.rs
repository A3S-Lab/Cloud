use super::archive::{ArchiveLimits, ArchiveSummary};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub(super) enum DirectoryArchiveError {
    #[error("Task output directory is invalid: {0}")]
    Invalid(String),
    #[error("Task output directory storage failed: {0}")]
    Storage(String),
}

pub(super) struct EncodedDirectoryArchive {
    pub(super) digest: String,
    pub(super) size_bytes: u64,
}

#[derive(Debug)]
enum DirectoryEntryKind {
    Directory,
    File { size_bytes: u64, executable: bool },
}

#[derive(Debug)]
struct DirectoryEntry {
    relative_path: PathBuf,
    source_path: PathBuf,
    kind: DirectoryEntryKind,
}

pub(super) fn encode_directory_archive(
    source: &Path,
    destination: &Path,
    limits: ArchiveLimits,
    maximum_archive_bytes: u64,
) -> Result<EncodedDirectoryArchive, DirectoryArchiveError> {
    validate_options(source, destination, limits, maximum_archive_bytes)?;
    let source = canonical_plain_directory(source)?;
    let mut entries = Vec::new();
    let mut summary = ArchiveSummary {
        entries: 0,
        expanded_bytes: 0,
    };
    collect_entries(&source, Path::new(""), limits, &mut summary, &mut entries)?;
    if entries.is_empty() {
        return Err(DirectoryArchiveError::Invalid(
            "Task output directory is empty".into(),
        ));
    }

    let result = write_archive(destination, entries, maximum_archive_bytes);
    if result.is_err() {
        let _ = std::fs::remove_file(destination);
    }
    result
}

fn validate_options(
    source: &Path,
    destination: &Path,
    limits: ArchiveLimits,
    maximum_archive_bytes: u64,
) -> Result<(), DirectoryArchiveError> {
    if source.as_os_str().is_empty()
        || destination.as_os_str().is_empty()
        || destination.exists()
        || destination.parent().is_none()
        || limits.max_entries == 0
        || limits.max_file_bytes == 0
        || limits.max_expanded_bytes == 0
        || limits.max_file_bytes > limits.max_expanded_bytes
        || maximum_archive_bytes == 0
    {
        return Err(DirectoryArchiveError::Invalid(
            "Task output archive options are invalid".into(),
        ));
    }
    Ok(())
}

fn canonical_plain_directory(source: &Path) -> Result<PathBuf, DirectoryArchiveError> {
    let metadata = std::fs::symlink_metadata(source).map_err(storage)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(unsupported_entry());
    }
    std::fs::canonicalize(source).map_err(storage)
}

fn collect_entries(
    root: &Path,
    relative: &Path,
    limits: ArchiveLimits,
    summary: &mut ArchiveSummary,
    entries: &mut Vec<DirectoryEntry>,
) -> Result<(), DirectoryArchiveError> {
    let directory = root.join(relative);
    let metadata = std::fs::symlink_metadata(&directory).map_err(storage)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(unsupported_entry());
    }
    let canonical = std::fs::canonicalize(&directory).map_err(storage)?;
    if !canonical.starts_with(root) {
        return Err(DirectoryArchiveError::Invalid(
            "Task output entry escapes its output root".into(),
        ));
    }

    let mut children = std::fs::read_dir(&directory)
        .map_err(storage)?
        .map(|entry| entry.map_err(storage))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child.file_name();
        let name = name.to_str().ok_or_else(|| {
            DirectoryArchiveError::Invalid("Task output entry name is not UTF-8".into())
        })?;
        if name.is_empty() || name.contains(['\0', '\r', '\n']) {
            return Err(DirectoryArchiveError::Invalid(
                "Task output entry name is invalid".into(),
            ));
        }
        let relative_path = relative.join(name);
        let source_path = root.join(&relative_path);
        let metadata = std::fs::symlink_metadata(&source_path).map_err(storage)?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            increment_entries(summary, limits)?;
            entries.push(DirectoryEntry {
                relative_path: relative_path.clone(),
                source_path,
                kind: DirectoryEntryKind::Directory,
            });
            collect_entries(root, &relative_path, limits, summary, entries)?;
        } else if metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
            let size_bytes = metadata.len();
            if size_bytes > limits.max_file_bytes {
                return Err(DirectoryArchiveError::Invalid(
                    "Task output file exceeds the single-file limit".into(),
                ));
            }
            summary.expanded_bytes =
                summary
                    .expanded_bytes
                    .checked_add(size_bytes)
                    .ok_or_else(|| {
                        DirectoryArchiveError::Invalid("Task output size overflowed".into())
                    })?;
            if summary.expanded_bytes > limits.max_expanded_bytes {
                return Err(DirectoryArchiveError::Invalid(
                    "Task output exceeds the expanded-byte limit".into(),
                ));
            }
            increment_entries(summary, limits)?;
            entries.push(DirectoryEntry {
                relative_path,
                source_path,
                kind: DirectoryEntryKind::File {
                    size_bytes,
                    executable: executable(&metadata),
                },
            });
        } else {
            return Err(unsupported_entry());
        }
    }
    Ok(())
}

fn increment_entries(
    summary: &mut ArchiveSummary,
    limits: ArchiveLimits,
) -> Result<(), DirectoryArchiveError> {
    if summary.entries >= limits.max_entries {
        return Err(DirectoryArchiveError::Invalid(
            "Task output exceeds the entry limit".into(),
        ));
    }
    summary.entries += 1;
    Ok(())
}

fn write_archive(
    destination: &Path,
    entries: Vec<DirectoryEntry>,
    maximum_archive_bytes: u64,
) -> Result<EncodedDirectoryArchive, DirectoryArchiveError> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(storage)?;
    let writer = BoundedDigestWriter::new(file, maximum_archive_bytes);
    let mut builder = tar::Builder::new(writer);
    for entry in entries {
        match entry.kind {
            DirectoryEntryKind::Directory => {
                let path = format!("{}/", archive_path(&entry.relative_path)?);
                let mut header = deterministic_header(0, 0o755);
                header.set_entry_type(tar::EntryType::Directory);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, io::empty())
                    .map_err(archive_error)?;
            }
            DirectoryEntryKind::File {
                size_bytes,
                executable,
            } => {
                let mut file = open_regular_file(&entry.source_path, size_bytes)?;
                let mut header =
                    deterministic_header(size_bytes, if executable { 0o755 } else { 0o644 });
                header.set_entry_type(tar::EntryType::Regular);
                header.set_cksum();
                builder
                    .append_data(&mut header, archive_path(&entry.relative_path)?, &mut file)
                    .map_err(archive_error)?;
                let metadata = file.metadata().map_err(storage)?;
                if !metadata.is_file() || metadata.len() != size_bytes {
                    return Err(DirectoryArchiveError::Invalid(
                        "Task output file changed while it was archived".into(),
                    ));
                }
            }
        }
    }
    builder.finish().map_err(archive_error)?;
    let mut writer = builder.into_inner().map_err(archive_error)?;
    writer.flush().map_err(storage)?;
    writer.file.sync_all().map_err(storage)?;
    Ok(writer.finish())
}

fn deterministic_header(size_bytes: u64, mode: u32) -> tar::Header {
    let mut header = tar::Header::new_gnu();
    header.set_size(size_bytes);
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header
}

fn archive_path(path: &Path) -> Result<&str, DirectoryArchiveError> {
    path.to_str().ok_or_else(|| {
        DirectoryArchiveError::Invalid("Task output archive path is not UTF-8".into())
    })
}

fn open_regular_file(path: &Path, expected_size: u64) -> Result<File, DirectoryArchiveError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(storage)?;
    let metadata = file.metadata().map_err(storage)?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(DirectoryArchiveError::Invalid(
            "Task output file changed before it was archived".into(),
        ));
    }
    Ok(file)
}

#[cfg(unix)]
fn executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn unsupported_entry() -> DirectoryArchiveError {
    DirectoryArchiveError::Invalid(
        "Task output accepts only plain directories and regular files".into(),
    )
}

fn archive_error(error: io::Error) -> DirectoryArchiveError {
    if error.kind() == io::ErrorKind::InvalidData {
        DirectoryArchiveError::Invalid(error.to_string())
    } else {
        storage(error)
    }
}

fn storage(error: io::Error) -> DirectoryArchiveError {
    DirectoryArchiveError::Storage(error.to_string())
}

struct BoundedDigestWriter {
    file: File,
    digest: Sha256,
    size_bytes: u64,
    maximum_bytes: u64,
}

impl BoundedDigestWriter {
    fn new(file: File, maximum_bytes: u64) -> Self {
        Self {
            file,
            digest: Sha256::new(),
            size_bytes: 0,
            maximum_bytes,
        }
    }

    fn finish(self) -> EncodedDirectoryArchive {
        EncodedDirectoryArchive {
            digest: format!("sha256:{:x}", self.digest.finalize()),
            size_bytes: self.size_bytes,
        }
    }
}

impl Write for BoundedDigestWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.maximum_bytes.saturating_sub(self.size_bytes);
        if buffer.len() as u64 > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Task output archive exceeds its maximum size",
            ));
        }
        let written = self.file.write(buffer)?;
        self.digest.update(&buffer[..written]);
        self.size_bytes = self
            .size_bytes
            .checked_add(written as u64)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "archive size overflowed"))?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
