use super::*;
use crate::modules::artifacts::application::IExternalSourceArchivePort;
use crate::modules::shared_kernel::domain::{BuildRunId, OrganizationId, SourceConnectionId};
use crate::modules::sources::domain::{
    CompleteGithubConnection, GithubAccountId, GithubAccountKind, GithubConnection,
    GithubConnectionFlow, GithubConnectionReconciled, GithubInstallationId,
    GithubInstallationTokenError, GithubInstallationTokenRequest, GithubLogin, NewGithubConnection,
    SourceProviderCredential,
};
use crate::modules::sources::infrastructure::persistence::InMemoryGithubConnectionRepository;
use crate::modules::sources::infrastructure::GithubInstallationTokenIssuer;
use crate::modules::sources::published::{GitProvider, GitRepository};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

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
    assert_eq!(checkout.credential_calls(), 0);
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
    assert_eq!(checkout.credential_calls(), 0);
    assert_eq!(std::fs::read_dir(root.path().join("staging"))?.count(), 0);
    Ok(())
}

#[tokio::test]
async fn private_checkout_uses_one_authoritative_credential_then_replays_offline(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source_directory = root.path().join("source");
    tokio::fs::create_dir(&source_directory).await?;
    tokio::fs::write(source_directory.join("Dockerfile"), "FROM scratch\n").await?;
    let request = request()?;
    let checkout = Arc::new(ReplayCheckout::private(checked_out_source(
        &request,
        source_directory,
    )));
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    connect(&connections, request.organization_id()).await?;
    let tokens = Arc::new(RecordingInstallationTokens::default());
    let adapter = ExternalSourceBuildArchiveAdapter::new(
        checkout.clone(),
        connections,
        tokens.clone(),
        root.path().join("staging"),
        1_024,
        16 * 1024 * 1024,
    )?;

    let _ = read_archive(adapter.prepare(request).await?).await?;
    assert_eq!(checkout.calls(), 3);
    assert_eq!(checkout.credential_calls(), 1);
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 1);
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
    ExternalSourceBuildArchiveAdapter::new(
        checkout,
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        root.join("staging"),
        1_024,
        16 * 1024 * 1024,
    )
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
    CheckedOutSource {
        checkout_id: request.build_run_id().as_uuid(),
        repository: request.repository().clone(),
        commit_sha: request.commit_sha().clone(),
        directory,
        git_tree_id: "1".repeat(40),
        content_digest: format!("sha256:{}", "2".repeat(64)),
        file_count: 2,
        content_bytes: 27,
    }
}

struct ReplayCheckout {
    source: CheckedOutSource,
    change_on_replay: bool,
    private: bool,
    calls: AtomicUsize,
    credential_calls: AtomicUsize,
    removals: AtomicUsize,
}

impl ReplayCheckout {
    fn new(source: CheckedOutSource, change_on_replay: bool) -> Self {
        Self {
            source,
            change_on_replay,
            private: false,
            calls: AtomicUsize::new(0),
            credential_calls: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
        }
    }

    fn private(source: CheckedOutSource) -> Self {
        Self {
            source,
            change_on_replay: false,
            private: true,
            calls: AtomicUsize::new(0),
            credential_calls: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn credential_calls(&self) -> usize {
        self.credential_calls.load(Ordering::SeqCst)
    }

    fn removals(&self) -> usize {
        self.removals.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ISourceCheckout for ReplayCheckout {
    async fn checkout(
        &self,
        request: &SourceCheckoutRequest,
        credential: Option<&SourceProviderCredential>,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if credential.is_some() {
            self.credential_calls.fetch_add(1, Ordering::SeqCst);
        }
        if self.private && call == 0 && credential.is_none() {
            return Err(SourceCheckoutError::Unavailable(
                "private source requires installation authority".into(),
            ));
        }
        if self.private && call == 1 && credential.is_none() {
            return Err(SourceCheckoutError::Conflict);
        }
        if request.checkout_id != self.source.checkout_id
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

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError> {
        if checkout_id != self.source.checkout_id {
            return Err(SourceCheckoutError::Conflict);
        }
        self.removals.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

async fn connect(
    repository: &Arc<InMemoryGithubConnectionRepository>,
    organization_id: OrganizationId,
) -> Result<GithubConnection, Box<dyn std::error::Error>> {
    let connected_at = chrono::Utc::now();
    let flow_id = Uuid::now_v7();
    let installation_id = GithubInstallationId::parse(42)?;
    let installation_state = format!("sha256:{}", "a".repeat(64));
    repository
        .begin_flow(GithubConnectionFlow::begin(
            flow_id,
            organization_id,
            installation_state.clone(),
            connected_at - chrono::Duration::minutes(2),
            connected_at + chrono::Duration::minutes(8),
        )?)
        .await?;
    repository
        .prepare_oauth(
            &installation_state,
            installation_id,
            format!("sha256:{}", "b".repeat(64)),
            format!("sha256:{}", "c".repeat(64)),
            connected_at - chrono::Duration::minutes(1),
        )
        .await?;
    let connection = GithubConnection::connect(NewGithubConnection {
        id: SourceConnectionId::new(),
        organization_id,
        installation_id,
        account_id: GithubAccountId::parse(100)?,
        account_login: GithubLogin::parse("A3S-Lab")?,
        account_kind: GithubAccountKind::Organization,
        verified_by_user_id: GithubAccountId::parse(200)?,
        verified_by_user_login: GithubLogin::parse("octocat")?,
        connected_at,
    })?;
    let event = GithubConnectionReconciled::envelope(&connection, Uuid::now_v7())?;
    Ok(repository
        .complete(CompleteGithubConnection {
            flow_id,
            connection,
            event,
            completed_at: connected_at,
        })
        .await?)
}

#[derive(Default)]
struct RecordingInstallationTokens {
    calls: AtomicUsize,
}

#[async_trait]
impl IGithubInstallationTokenService for RecordingInstallationTokens {
    async fn issue(
        &self,
        request: GithubInstallationTokenRequest,
    ) -> Result<SourceProviderCredential, GithubInstallationTokenError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        SourceProviderCredential::new(
            &request.repository,
            Zeroizing::new("installation-token".into()),
            request.requested_at,
            request.requested_at + chrono::Duration::minutes(10),
        )
        .map_err(GithubInstallationTokenError::Protocol)
    }
}
