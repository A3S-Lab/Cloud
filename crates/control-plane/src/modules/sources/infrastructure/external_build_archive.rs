use crate::modules::artifacts::application::{
    BuildInputPreparationError, ExternalSourceArchiveRequest, IExternalSourceArchivePort,
    OpenExternalSourceArchive,
};
use crate::modules::shared_kernel::domain::{BuildRunId, GitCommitSha, Sha256Digest};
use crate::modules::sources::domain::{
    CheckedOutSource, GithubInstallationTokenRequest, IGithubConnectionRepository,
    IGithubInstallationTokenService, ISourceCheckout, SourceCheckoutError, SourceCheckoutRequest,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};
use uuid::Uuid;

const MAX_ARCHIVE_ENTRIES: usize = 2_000_000;
const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

/// Sources-owned adapter for the Artifacts external-source archive port.
///
/// Provider credentials, checkout receipts, local directories, and packaging
/// policy remain inside Sources. Artifacts receives only immutable digests and
/// a bounded byte stream.
pub struct ExternalSourceBuildArchiveAdapter {
    checkout: Arc<dyn ISourceCheckout>,
    connections: Arc<dyn IGithubConnectionRepository>,
    installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    staging_root: PathBuf,
    max_entries: usize,
    max_archive_bytes: u64,
}

impl ExternalSourceBuildArchiveAdapter {
    pub fn new(
        checkout: Arc<dyn ISourceCheckout>,
        connections: Arc<dyn IGithubConnectionRepository>,
        installation_tokens: Arc<dyn IGithubInstallationTokenService>,
        staging_root: impl Into<PathBuf>,
        max_entries: usize,
        max_archive_bytes: u64,
    ) -> Result<Self, String> {
        let staging_root = staging_root.into();
        validate_root(&staging_root, "external Source archive staging root")?;
        if max_entries == 0
            || max_entries > MAX_ARCHIVE_ENTRIES
            || max_archive_bytes == 0
            || max_archive_bytes > MAX_ARCHIVE_BYTES
        {
            return Err("external Source archive limits are invalid".into());
        }
        Ok(Self {
            checkout,
            connections,
            installation_tokens,
            staging_root,
            max_entries,
            max_archive_bytes,
        })
    }

    async fn checkout(
        &self,
        request: &ExternalSourceArchiveRequest,
    ) -> Result<(SourceCheckoutRequest, CheckedOutSource), BuildInputPreparationError> {
        let checkout_request = SourceCheckoutRequest::new(
            request.build_run_id().as_uuid(),
            request.repository().clone(),
            request.commit_sha().clone(),
        )
        .map_err(BuildInputPreparationError::Invalid)?;
        let checked_out = match self.checkout.checkout(&checkout_request, None).await {
            Ok(source) => source,
            Err(SourceCheckoutError::Unavailable(_)) => {
                let connection = self
                    .connections
                    .find(request.organization_id())
                    .await
                    .map_err(|error| {
                        BuildInputPreparationError::Unavailable(format!(
                            "source connection lookup failed: {error}"
                        ))
                    })?
                    .filter(|connection| {
                        connection.organization_id == request.organization_id()
                            && connection.is_authoritative()
                    })
                    .ok_or_else(|| {
                        BuildInputPreparationError::Unavailable(
                            "source repository has no active installation authority".into(),
                        )
                    })?;
                let credential = self
                    .installation_tokens
                    .issue(GithubInstallationTokenRequest {
                        organization_id: connection.organization_id,
                        connection_id: connection.id,
                        installation_id: connection.installation_id,
                        repository: request.repository().clone(),
                        requested_at: chrono::Utc::now(),
                    })
                    .await
                    .map_err(|_| {
                        BuildInputPreparationError::Unavailable(
                            "source repository credential is unavailable".into(),
                        )
                    })?;
                self.checkout
                    .checkout(&checkout_request, Some(&credential))
                    .await
                    .map_err(map_checkout_error)?
            }
            Err(error) => return Err(map_checkout_error(error)),
        };
        validate_checkout(&checkout_request, &checked_out)?;
        Ok((checkout_request, checked_out))
    }

    async fn package(
        &self,
        build_run_id: BuildRunId,
        source: &CheckedOutSource,
    ) -> Result<(PathBuf, Sha256Digest, u64), BuildInputPreparationError> {
        let root = ensure_staging_root(&self.staging_root).await?;
        let staging = root.join(format!("{build_run_id}-{}.tar", Uuid::now_v7()));
        let source_directory = source.directory.clone();
        let archive_path = staging.clone();
        let max_entries = self.max_entries;
        let max_archive_bytes = self.max_archive_bytes;
        let archived = tokio::task::spawn_blocking(move || {
            write_directory_archive(
                &source_directory,
                &archive_path,
                max_entries,
                max_archive_bytes,
            )
        })
        .await;
        let archived = match archived {
            Ok(result) => result,
            Err(error) => {
                let _ = tokio::fs::remove_file(&staging).await;
                return Err(BuildInputPreparationError::Storage(format!(
                    "external Source archive task failed: {error}"
                )));
            }
        };
        match archived {
            Ok((digest, size_bytes)) => match Sha256Digest::parse(digest) {
                Ok(digest) => Ok((staging, digest, size_bytes)),
                Err(error) => {
                    let _ = tokio::fs::remove_file(&staging).await;
                    Err(BuildInputPreparationError::Integrity(error))
                }
            },
            Err(error) => {
                let _ = tokio::fs::remove_file(&staging).await;
                Err(error)
            }
        }
    }
}

#[async_trait]
impl IExternalSourceArchivePort for ExternalSourceBuildArchiveAdapter {
    async fn prepare(
        &self,
        request: ExternalSourceArchiveRequest,
    ) -> Result<OpenExternalSourceArchive, BuildInputPreparationError> {
        request
            .validate()
            .map_err(BuildInputPreparationError::Invalid)?;
        let (checkout_request, checkout) = self.checkout(&request).await?;
        let (archive_path, archive_digest, size_bytes) =
            self.package(request.build_run_id(), &checkout).await?;

        // This credential-free replay rehashes the owner checkout immediately
        // after packaging. The returned archive file is immutable and no
        // provider state is exposed while Artifacts admits its bytes.
        let replay = match self.checkout.checkout(&checkout_request, None).await {
            Ok(replay) => replay,
            Err(error) => {
                let _ = tokio::fs::remove_file(&archive_path).await;
                return Err(map_checkout_error(error));
            }
        };
        if validate_checkout(&checkout_request, &replay).is_err() || replay != checkout {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(BuildInputPreparationError::Integrity(
                "source checkout identity changed while packaging build input".into(),
            ));
        }
        let source_content_digest = match Sha256Digest::parse(checkout.content_digest) {
            Ok(digest) => digest,
            Err(error) => {
                let _ = tokio::fs::remove_file(&archive_path).await;
                return Err(BuildInputPreparationError::Integrity(error));
            }
        };
        let reader = TemporaryArchiveReader::open(archive_path).await?;
        OpenExternalSourceArchive::new(
            source_content_digest,
            archive_digest,
            size_bytes,
            Box::pin(reader),
        )
        .map_err(BuildInputPreparationError::Integrity)
    }

    async fn remove(&self, build_run_id: BuildRunId) -> Result<(), BuildInputPreparationError> {
        if build_run_id.as_uuid().is_nil() {
            return Err(BuildInputPreparationError::Invalid(
                "external Source archive BuildRun ID is invalid".into(),
            ));
        }
        self.checkout
            .remove(build_run_id.as_uuid())
            .await
            .map_err(map_checkout_error)
    }
}

fn validate_checkout(
    request: &SourceCheckoutRequest,
    source: &CheckedOutSource,
) -> Result<(), BuildInputPreparationError> {
    if source.checkout_id != request.checkout_id
        || source.repository != request.repository
        || source.commit_sha != request.commit_sha
        || source.directory.as_os_str().is_empty()
    {
        return Err(BuildInputPreparationError::Integrity(
            "source checkout receipt differs from its exact request".into(),
        ));
    }
    GitCommitSha::parse(&source.git_tree_id).map_err(BuildInputPreparationError::Integrity)?;
    Sha256Digest::parse(&source.content_digest).map_err(BuildInputPreparationError::Integrity)?;
    Ok(())
}

fn map_checkout_error(error: SourceCheckoutError) -> BuildInputPreparationError {
    match error {
        SourceCheckoutError::Invalid(message) => BuildInputPreparationError::Invalid(message),
        SourceCheckoutError::Conflict => BuildInputPreparationError::Conflict,
        SourceCheckoutError::Unavailable(message) => {
            BuildInputPreparationError::Unavailable(message)
        }
        SourceCheckoutError::Integrity(message) => BuildInputPreparationError::Integrity(message),
        SourceCheckoutError::Storage(message) => BuildInputPreparationError::Storage(message),
    }
}

struct TemporaryArchiveReader {
    file: Option<tokio::fs::File>,
    path: PathBuf,
}

impl TemporaryArchiveReader {
    async fn open(path: PathBuf) -> Result<Self, BuildInputPreparationError> {
        let file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) => {
                let _ = tokio::fs::remove_file(&path).await;
                return Err(BuildInputPreparationError::Storage(format!(
                    "could not open external Source archive: {error}"
                )));
            }
        };
        Ok(Self {
            file: Some(file),
            path,
        })
    }
}

impl AsyncRead for TemporaryArchiveReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(
            self.file
                .as_mut()
                .expect("temporary archive reader is live until drop"),
        )
        .poll_read(context, buffer)
    }
}

impl Drop for TemporaryArchiveReader {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn ensure_staging_root(root: &Path) -> Result<PathBuf, BuildInputPreparationError> {
    tokio::fs::create_dir_all(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not create external Source archive staging root: {error}"
        ))
    })?;
    let metadata = tokio::fs::symlink_metadata(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not inspect external Source archive staging root: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BuildInputPreparationError::Integrity(
            "external Source archive staging root is not an owned directory".into(),
        ));
    }
    tokio::fs::canonicalize(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not canonicalize external Source archive staging root: {error}"
        ))
    })
}

fn write_directory_archive(
    source: &Path,
    destination: &Path,
    max_entries: usize,
    max_archive_bytes: u64,
) -> Result<(String, u64), BuildInputPreparationError> {
    let metadata = std::fs::symlink_metadata(source).map_err(storage_io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BuildInputPreparationError::Integrity(
            "checked-out source is not an owned directory".into(),
        ));
    }
    let source = source.canonicalize().map_err(storage_io)?;
    let mut entries = Vec::new();
    collect_entries(&source, Path::new(""), &mut entries, max_entries)?;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(storage_io)?;
    let writer = BoundedHashingWriter::new(file, max_archive_bytes);
    let mut archive = tar::Builder::new(writer);
    archive.mode(tar::HeaderMode::Deterministic);
    for entry in entries {
        append_entry(&mut archive, &source, &entry)?;
    }
    archive.finish().map_err(storage_io)?;
    let writer = archive.into_inner().map_err(storage_io)?;
    writer.finish()
}

#[derive(Clone)]
struct ArchiveEntry {
    path: PathBuf,
    kind: ArchiveEntryKind,
}

#[derive(Clone)]
enum ArchiveEntryKind {
    Directory,
    File { size: u64, executable: bool },
    Symlink { target: PathBuf },
}

fn collect_entries(
    root: &Path,
    relative: &Path,
    entries: &mut Vec<ArchiveEntry>,
    max_entries: usize,
) -> Result<(), BuildInputPreparationError> {
    let directory = root.join(relative);
    let mut children = std::fs::read_dir(&directory)
        .map_err(storage_io)?
        .map(|entry| entry.map_err(storage_io))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let name = child.file_name();
        let path = relative.join(name);
        validate_archive_path(&path)?;
        let metadata = std::fs::symlink_metadata(child.path()).map_err(storage_io)?;
        let kind = if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(child.path()).map_err(storage_io)?;
            validate_symlink_target(&path, &target)?;
            ArchiveEntryKind::Symlink { target }
        } else if metadata.is_dir() {
            ArchiveEntryKind::Directory
        } else if metadata.is_file() {
            ArchiveEntryKind::File {
                size: metadata.len(),
                executable: is_executable(&metadata),
            }
        } else {
            return Err(BuildInputPreparationError::Integrity(
                "checked-out source contains an unsupported filesystem entry".into(),
            ));
        };
        entries.push(ArchiveEntry {
            path: path.clone(),
            kind: kind.clone(),
        });
        if entries.len() > max_entries {
            return Err(BuildInputPreparationError::Invalid(
                "external Source archive exceeds its entry bound".into(),
            ));
        }
        if matches!(kind, ArchiveEntryKind::Directory) {
            collect_entries(root, &path, entries, max_entries)?;
        }
    }
    Ok(())
}

fn append_entry<W: Write>(
    archive: &mut tar::Builder<W>,
    root: &Path,
    entry: &ArchiveEntry,
) -> Result<(), BuildInputPreparationError> {
    let mut header = tar::Header::new_gnu();
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    match &entry.kind {
        ArchiveEntryKind::Directory => {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_mode(0o555);
            header.set_size(0);
            header.set_cksum();
            archive
                .append_data(&mut header, &entry.path, io::empty())
                .map_err(storage_io)
        }
        ArchiveEntryKind::File { size, executable } => {
            let path = root.join(&entry.path);
            let file = File::open(&path).map_err(storage_io)?;
            let metadata = file.metadata().map_err(storage_io)?;
            if !metadata.is_file()
                || metadata.len() != *size
                || is_executable(&metadata) != *executable
            {
                return Err(BuildInputPreparationError::Integrity(
                    "checked-out source changed while creating its archive".into(),
                ));
            }
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(if *executable { 0o555 } else { 0o444 });
            header.set_size(*size);
            header.set_cksum();
            archive
                .append_data(&mut header, &entry.path, file.take(*size))
                .map_err(storage_io)
        }
        ArchiveEntryKind::Symlink { target } => {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_mode(0o777);
            header.set_size(0);
            header.set_link_name(target).map_err(storage_io)?;
            header.set_cksum();
            archive
                .append_data(&mut header, &entry.path, io::empty())
                .map_err(storage_io)
        }
    }
}

fn validate_archive_path(path: &Path) -> Result<(), BuildInputPreparationError> {
    let text = path.to_str().ok_or_else(|| {
        BuildInputPreparationError::Integrity("checked-out source path must be UTF-8".into())
    })?;
    if text.is_empty()
        || text.len() > 4096
        || text.contains(['\0', '\r', '\n'])
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BuildInputPreparationError::Integrity(
            "checked-out source path is unsafe".into(),
        ));
    }
    Ok(())
}

fn validate_symlink_target(path: &Path, target: &Path) -> Result<(), BuildInputPreparationError> {
    if target.is_absolute() || target.as_os_str().is_empty() {
        return Err(BuildInputPreparationError::Integrity(
            "checked-out source symlink escapes its root".into(),
        ));
    }
    let mut depth = path
        .parent()
        .map_or(0, |parent| parent.components().count());
    for component in target.components() {
        match component {
            Component::Normal(_) => depth = depth.saturating_add(1),
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BuildInputPreparationError::Integrity(
                    "checked-out source symlink escapes its root".into(),
                ))
            }
        }
    }
    Ok(())
}

fn validate_root(path: &Path, label: &str) -> Result<(), String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("{label} must be UTF-8"))?;
    if text.trim().is_empty()
        || text.len() > 4096
        || text.contains(['\0', '\r', '\n'])
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn storage_io(error: io::Error) -> BuildInputPreparationError {
    BuildInputPreparationError::Storage(format!(
        "could not create external Source archive: {error}"
    ))
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

struct BoundedHashingWriter {
    file: File,
    digest: Sha256,
    size: u64,
    maximum: u64,
}

impl BoundedHashingWriter {
    fn new(file: File, maximum: u64) -> Self {
        Self {
            file,
            digest: Sha256::new(),
            size: 0,
            maximum,
        }
    }

    fn finish(self) -> Result<(String, u64), BuildInputPreparationError> {
        self.file.sync_all().map_err(storage_io)?;
        Ok((format!("sha256:{:x}", self.digest.finalize()), self.size))
    }
}

impl Write for BoundedHashingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let next = self
            .size
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| io::Error::other("external Source archive size overflowed"))?;
        if next > self.maximum {
            return Err(io::Error::other(
                "external Source archive exceeds its byte bound",
            ));
        }
        let written = self.file.write(buffer)?;
        self.digest.update(&buffer[..written]);
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
#[path = "external_build_archive_tests.rs"]
mod tests;
