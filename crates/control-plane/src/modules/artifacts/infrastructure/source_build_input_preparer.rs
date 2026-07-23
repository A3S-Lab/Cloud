use crate::modules::artifacts::domain::{
    BuildArtifact, BuildInputPreparationError, BuildRun, IBuildInputPreparer, INodeArtifactStore,
    NodeArtifactDescriptor, NodeArtifactStoreError, PreparedBuildInput,
};
use crate::modules::sources::domain::{
    CheckedOutSource, ExternalSourceRevision, GithubInstallationTokenRequest,
    IGithubConnectionRepository, IGithubInstallationTokenService, ISourceCheckout,
    SourceCheckoutError, SourceCheckoutRequest,
};
use a3s_cloud_contracts::{artifact_uri, NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE};
use a3s_runtime::contract::ArtifactRef;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const MAX_ARCHIVE_ENTRIES: usize = 2_000_000;
const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

pub struct SourceBuildInputPreparer {
    checkout: Arc<dyn ISourceCheckout>,
    connections: Arc<dyn IGithubConnectionRepository>,
    installation_tokens: Arc<dyn IGithubInstallationTokenService>,
    artifacts: Arc<dyn INodeArtifactStore>,
    staging_root: PathBuf,
    max_entries: usize,
    max_archive_bytes: u64,
}

impl SourceBuildInputPreparer {
    pub fn new(
        checkout: Arc<dyn ISourceCheckout>,
        connections: Arc<dyn IGithubConnectionRepository>,
        installation_tokens: Arc<dyn IGithubInstallationTokenService>,
        artifacts: Arc<dyn INodeArtifactStore>,
        staging_root: impl Into<PathBuf>,
        max_entries: usize,
        max_archive_bytes: u64,
    ) -> Result<Self, String> {
        let staging_root = staging_root.into();
        validate_root(&staging_root, "build input staging root")?;
        if max_entries == 0
            || max_entries > MAX_ARCHIVE_ENTRIES
            || max_archive_bytes == 0
            || max_archive_bytes > MAX_ARCHIVE_BYTES
        {
            return Err("build input archive limits are invalid".into());
        }
        Ok(Self {
            checkout,
            connections,
            installation_tokens,
            artifacts,
            staging_root,
            max_entries,
            max_archive_bytes,
        })
    }

    async fn checkout(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
    ) -> Result<(SourceCheckoutRequest, CheckedOutSource), BuildInputPreparationError> {
        let request = SourceCheckoutRequest::new(
            build.id.as_uuid(),
            revision.repository.clone(),
            revision.commit_sha.clone(),
        )
        .map_err(BuildInputPreparationError::Invalid)?;
        match self.checkout.checkout(&request, None).await {
            Ok(source) => Ok((request, source)),
            Err(SourceCheckoutError::Unavailable(_)) => {
                let connection = self
                    .connections
                    .find(build.organization_id)
                    .await
                    .map_err(|error| {
                        BuildInputPreparationError::Unavailable(format!(
                            "source connection lookup failed: {error}"
                        ))
                    })?
                    .filter(|connection| {
                        connection.organization_id == build.organization_id
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
                        repository: revision.repository.clone(),
                        requested_at: chrono::Utc::now(),
                    })
                    .await
                    .map_err(|_| {
                        BuildInputPreparationError::Unavailable(
                            "source repository credential is unavailable".into(),
                        )
                    })?;
                self.checkout
                    .checkout(&request, Some(&credential))
                    .await
                    .map(|source| (request, source))
                    .map_err(map_checkout_error)
            }
            Err(error) => Err(map_checkout_error(error)),
        }
    }

    async fn package(
        &self,
        build: &BuildRun,
        source: &CheckedOutSource,
    ) -> Result<BuildArtifact, BuildInputPreparationError> {
        let root = ensure_staging_root(&self.staging_root).await?;
        let staging = root.join(format!("{}-{}.tar", build.id, Uuid::now_v7()));
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
        .await
        .map_err(|error| {
            BuildInputPreparationError::Storage(format!("build input archive task failed: {error}"))
        })?;
        let (digest, size_bytes) = match archived {
            Ok(value) => value,
            Err(error) => {
                let _ = tokio::fs::remove_file(&staging).await;
                return Err(error);
            }
        };
        let artifact = ArtifactRef {
            uri: artifact_uri(&digest).map_err(BuildInputPreparationError::Invalid)?,
            digest,
            media_type: NODE_DIRECTORY_ARTIFACT_MEDIA_TYPE.into(),
        };
        let descriptor = NodeArtifactDescriptor::new(artifact, size_bytes)
            .map_err(BuildInputPreparationError::Invalid)?;
        let file = tokio::fs::File::open(&staging).await.map_err(|error| {
            BuildInputPreparationError::Storage(format!(
                "could not reopen build input archive: {error}"
            ))
        })?;
        let stored = self
            .artifacts
            .put(&descriptor, Box::pin(file))
            .await
            .map_err(map_artifact_error);
        let _ = tokio::fs::remove_file(&staging).await;
        let stored = stored?;
        BuildArtifact::new(
            stored.descriptor.artifact.uri,
            stored.descriptor.artifact.digest,
            stored.descriptor.artifact.media_type,
            stored.descriptor.size_bytes,
        )
        .map_err(BuildInputPreparationError::Invalid)
    }
}

#[async_trait]
impl IBuildInputPreparer for SourceBuildInputPreparer {
    async fn prepare(
        &self,
        build: &BuildRun,
        revision: &ExternalSourceRevision,
    ) -> Result<PreparedBuildInput, BuildInputPreparationError> {
        validate_identity(build, revision)?;
        let (request, source) = self.checkout(build, revision).await?;
        let artifact = self.package(build, &source).await?;

        // The second checkout call is an offline receipt replay. It closes the
        // package-time race by rehashing the credential-free worktree after the
        // immutable archive bytes have been admitted.
        let replay = self
            .checkout
            .checkout(&request, None)
            .await
            .map_err(map_checkout_error)?;
        if replay != source {
            return Err(BuildInputPreparationError::Integrity(
                "source checkout identity changed while packaging build input".into(),
            ));
        }
        Ok(PreparedBuildInput {
            source_content_digest: source.content_digest,
            artifact,
        })
    }

    async fn remove(&self, build: &BuildRun) -> Result<(), BuildInputPreparationError> {
        self.checkout
            .remove(build.id.as_uuid())
            .await
            .map_err(map_checkout_error)
    }
}

fn validate_identity(
    build: &BuildRun,
    revision: &ExternalSourceRevision,
) -> Result<(), BuildInputPreparationError> {
    if build.organization_id != revision.organization_id
        || build.project_id != revision.project_id
        || build.environment_id != revision.environment_id
        || build.source_revision_id != revision.id
    {
        return Err(BuildInputPreparationError::Conflict);
    }
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

fn map_artifact_error(error: NodeArtifactStoreError) -> BuildInputPreparationError {
    match error {
        NodeArtifactStoreError::Invalid(message) => BuildInputPreparationError::Invalid(message),
        NodeArtifactStoreError::Conflict => BuildInputPreparationError::Conflict,
        NodeArtifactStoreError::Integrity(message) => {
            BuildInputPreparationError::Integrity(message)
        }
        NodeArtifactStoreError::NotFound => {
            BuildInputPreparationError::Storage("admitted build input disappeared".into())
        }
        NodeArtifactStoreError::Storage(message) => BuildInputPreparationError::Storage(message),
    }
}

async fn ensure_staging_root(root: &Path) -> Result<PathBuf, BuildInputPreparationError> {
    tokio::fs::create_dir_all(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not create build input staging root: {error}"
        ))
    })?;
    let metadata = tokio::fs::symlink_metadata(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not inspect build input staging root: {error}"
        ))
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BuildInputPreparationError::Integrity(
            "build input staging root is not an owned directory".into(),
        ));
    }
    tokio::fs::canonicalize(root).await.map_err(|error| {
        BuildInputPreparationError::Storage(format!(
            "could not canonicalize build input staging root: {error}"
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
                "build input archive exceeds its entry bound".into(),
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
    BuildInputPreparationError::Storage(format!("could not create build input archive: {error}"))
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
            .ok_or_else(|| io::Error::other("build input archive size overflowed"))?;
        if next > self.maximum {
            return Err(io::Error::other(
                "build input archive exceeds its byte bound",
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
#[path = "source_build_input_preparer_tests.rs"]
mod tests;
