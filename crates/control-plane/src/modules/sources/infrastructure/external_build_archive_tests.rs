use super::*;
use crate::modules::artifacts::application::IExternalSourceArchivePort;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId, Sha256Digest};
use crate::modules::sources::domain::GitCommitSha;
use crate::modules::sources::domain::{CheckedOutSourceEntry, CheckedOutSourceEntryKind};
use crate::modules::sources::published::{GitProvider, GitRepository};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn archive_is_deterministic_replay_safe_and_credential_free(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source_directory = root.path().join("source");
    tokio::fs::create_dir(&source_directory).await?;
    tokio::fs::write(source_directory.join("Dockerfile"), "FROM scratch\n").await?;
    tokio::fs::write(source_directory.join("message.txt"), "deterministic\n").await?;
    let request = request()?;
    let checkout = Arc::new(ReplayCheckout::new(
        checked_out_source(&request, source_directory),
        false,
    ));
    let adapter = adapter(root.path(), checkout.clone())?;

    let (first_digest, first_bytes) = read_archive(adapter.prepare(request.clone()).await?).await?;
    let (replay_digest, replay_bytes) =
        read_archive(adapter.prepare(request.clone()).await?).await?;
    assert_eq!(first_digest, replay_digest);
    assert_eq!(first_bytes, replay_bytes);
    assert_eq!(
        first_digest,
        Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(&first_bytes)))?
    );
    assert_eq!(checkout.calls(), 4);
    adapter.remove(request.build_run_id()).await?;
    assert_eq!(checkout.removals(), 1);
    assert_eq!(
        std::fs::read_dir(root.path().join("staging"))?.count(),
        0,
        "temporary archive reader must remove its private file on drop"
    );
    Ok(())
}

#[tokio::test]
async fn package_time_checkout_change_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source_directory = root.path().join("source");
    tokio::fs::create_dir(&source_directory).await?;
    tokio::fs::write(source_directory.join("Dockerfile"), "FROM scratch\n").await?;
    let request = request()?;
    let checkout = Arc::new(ReplayCheckout::new(
        checked_out_source(&request, source_directory),
        true,
    ));
    let adapter = adapter(root.path(), checkout.clone())?;

    assert!(matches!(
        adapter.prepare(request).await,
        Err(BuildInputPreparationError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 2);
    assert_eq!(std::fs::read_dir(root.path().join("staging"))?.count(), 0);
    Ok(())
}

async fn read_archive(
    archive: OpenExternalSourceArchive,
) -> Result<(Sha256Digest, Vec<u8>), Box<dyn std::error::Error>> {
    let digest = archive.archive_digest().clone();
    let expected_size = archive.size_bytes();
    let mut reader = archive.into_reader();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    drop(reader);
    assert_eq!(bytes.len() as u64, expected_size);
    Ok((digest, bytes))
}

fn adapter(
    root: &Path,
    checkout: Arc<ReplayCheckout>,
) -> Result<ExternalSourceBuildArchiveAdapter, String> {
    ExternalSourceBuildArchiveAdapter::new(checkout, root.join("staging"), 1_024, 16 * 1024 * 1024)
}

fn request() -> Result<ExternalSourceArchiveRequest, String> {
    ExternalSourceArchiveRequest::new(
        OrganizationId::new(),
        BuildRunId::new(),
        GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?,
        GitCommitSha::parse("a".repeat(40))?,
    )
}

fn checked_out_source(
    request: &ExternalSourceArchiveRequest,
    directory: PathBuf,
) -> CheckedOutSource {
    let mut paths = std::fs::read_dir(&directory)
        .expect("checked-out fixture directory")
        .map(|entry| {
            entry
                .expect("checked-out fixture directory entry")
                .file_name()
                .into_string()
                .expect("UTF-8 fixture path")
        })
        .collect::<Vec<_>>();
    paths.sort();
    let entries = paths
        .iter()
        .map(|path| checked_out_entry(&directory, path))
        .collect::<Vec<_>>();
    let content_bytes = entries.iter().map(CheckedOutSourceEntry::size_bytes).sum();
    CheckedOutSource {
        checkout_id: request.build_run_id().as_uuid(),
        repository: request.repository().clone(),
        commit_sha: request.commit_sha().clone(),
        directory,
        git_tree_id: "1".repeat(40),
        content_digest: format!("sha256:{}", "2".repeat(64)),
        file_count: entries.len(),
        content_bytes,
        entries,
    }
}

fn checked_out_entry(directory: &Path, path: &str) -> CheckedOutSourceEntry {
    let content = std::fs::read(directory.join(path)).expect("checked-out fixture entry");
    CheckedOutSourceEntry::new(
        path,
        CheckedOutSourceEntryKind::Regular,
        content.len() as u64,
        Sha256Digest::from_bytes(&content),
    )
    .expect("checked-out fixture entry")
}

struct ReplayCheckout {
    source: CheckedOutSource,
    change_on_replay: bool,
    calls: AtomicUsize,
    removals: AtomicUsize,
}

impl ReplayCheckout {
    fn new(source: CheckedOutSource, change_on_replay: bool) -> Self {
        Self {
            source,
            change_on_replay,
            calls: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn removals(&self) -> usize {
        self.removals.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl IAuthorizedSourceCheckout for ReplayCheckout {
    async fn checkout(
        &self,
        organization_id: OrganizationId,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if organization_id.as_uuid().is_nil()
            || request.checkout_id != self.source.checkout_id
            || request.repository != self.source.repository
            || request.commit_sha != self.source.commit_sha
        {
            return Err(SourceCheckoutError::Conflict);
        }
        let mut source = self.source.clone();
        if self.change_on_replay && call == 1 {
            source.content_digest = format!("sha256:{}", "3".repeat(64));
        }
        Ok(source)
    }

    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if request.checkout_id != self.source.checkout_id
            || request.repository != self.source.repository
            || request.commit_sha != self.source.commit_sha
            || call == 0
        {
            return Err(SourceCheckoutError::Conflict);
        }
        let mut source = self.source.clone();
        if self.change_on_replay {
            source.content_digest = format!("sha256:{}", "3".repeat(64));
        }
        Ok(source)
    }

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError> {
        if checkout_id != self.source.checkout_id {
            return Err(SourceCheckoutError::Conflict);
        }
        self.removals.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
