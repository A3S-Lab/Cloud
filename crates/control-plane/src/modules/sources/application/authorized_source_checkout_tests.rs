use super::*;
use crate::modules::shared_kernel::domain::{Sha256Digest, SourceConnectionId};
use crate::modules::sources::application::SourceRepositoryCredentialService;
use crate::modules::sources::domain::{
    CheckedOutSourceEntry, CheckedOutSourceEntryKind, CompleteGithubConnection, GithubAccountId,
    GithubAccountKind, GithubConnection, GithubConnectionFlow, GithubConnectionReconciled,
    GithubInstallationId, GithubInstallationTokenError, GithubInstallationTokenRequest,
    GithubLogin, IGithubConnectionRepository, IGithubInstallationTokenService, NewGithubConnection,
    SourceProviderCredential,
};
use crate::modules::sources::infrastructure::persistence::InMemoryGithubConnectionRepository;
use crate::modules::sources::infrastructure::GithubInstallationTokenIssuer;
use crate::modules::sources::published::{GitProvider, GitRepository};
use std::sync::atomic::{AtomicUsize, Ordering};
use zeroize::Zeroizing;

#[tokio::test]
async fn public_checkout_bypasses_provider_credentials_and_validates_the_receipt(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let raw = Arc::new(RecordingCheckout::new(fixture.source.clone(), false));
    let service = AuthorizedSourceCheckoutService::new(
        raw.clone(),
        Arc::new(SourceRepositoryCredentialService::new(
            Arc::new(InMemoryGithubConnectionRepository::new()),
            Arc::new(GithubInstallationTokenIssuer::disabled()),
        )),
    );

    let checked_out = service
        .checkout(fixture.organization_id, &fixture.request)
        .await?;

    assert_eq!(checked_out, fixture.source);
    assert_eq!(raw.calls(), 1);
    assert_eq!(raw.credential_calls(), 0);
    assert_eq!(
        service.replay(&fixture.request).await?,
        fixture.source,
        "strict replay returns the committed checkout"
    );
    assert_eq!(raw.calls(), 1);
    assert_eq!(raw.replays(), 1);
    assert_eq!(raw.credential_calls(), 0);
    service.remove(fixture.request.checkout_id).await?;
    assert_eq!(raw.removals.load(Ordering::SeqCst), 1);
    assert!(matches!(
        service.remove(Uuid::nil()).await,
        Err(SourceCheckoutError::Invalid(_))
    ));
    assert_eq!(raw.removals.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn private_checkout_uses_one_authoritative_credential_then_replays_without_it(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let raw = Arc::new(RecordingCheckout::new(fixture.source.clone(), true));
    let connections = Arc::new(InMemoryGithubConnectionRepository::new());
    connect(&connections, fixture.organization_id).await?;
    let tokens = Arc::new(RecordingInstallationTokens::default());
    let service = AuthorizedSourceCheckoutService::new(
        raw.clone(),
        Arc::new(SourceRepositoryCredentialService::new(
            connections,
            tokens.clone(),
        )),
    );

    assert_eq!(
        service
            .checkout(fixture.organization_id, &fixture.request)
            .await?,
        fixture.source
    );
    assert_eq!(service.replay(&fixture.request).await?, fixture.source);
    assert_eq!(raw.calls(), 2);
    assert_eq!(raw.replays(), 1);
    assert_eq!(raw.credential_calls(), 1);
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn private_checkout_without_repository_authority_is_concealed(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let raw = Arc::new(RecordingCheckout::new(fixture.source, true));
    let service = AuthorizedSourceCheckoutService::new(
        raw.clone(),
        Arc::new(SourceRepositoryCredentialService::new(
            Arc::new(InMemoryGithubConnectionRepository::new()),
            Arc::new(GithubInstallationTokenIssuer::disabled()),
        )),
    );

    assert!(matches!(
        service
            .checkout(fixture.organization_id, &fixture.request)
            .await,
        Err(SourceCheckoutError::Unavailable(_))
    ));
    assert_eq!(raw.calls(), 1);
    assert_eq!(raw.credential_calls(), 0);
    Ok(())
}

#[tokio::test]
async fn inconsistent_checkout_receipt_fails_inside_the_sources_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut source = fixture.source.clone();
    source.file_count += 1;
    let raw = Arc::new(RecordingCheckout::new(source, false));
    let tokens = Arc::new(RecordingInstallationTokens::default());
    let service = AuthorizedSourceCheckoutService::new(
        raw.clone(),
        Arc::new(SourceRepositoryCredentialService::new(
            Arc::new(InMemoryGithubConnectionRepository::new()),
            tokens.clone(),
        )),
    );

    assert!(matches!(
        service
            .checkout(fixture.organization_id, &fixture.request)
            .await,
        Err(SourceCheckoutError::Integrity(_))
    ));
    assert_eq!(tokens.calls.load(Ordering::SeqCst), 0);
    assert_eq!(raw.removals.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn authorized_checkout_service_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AuthorizedSourceCheckoutService>();
}

struct Fixture {
    _root: tempfile::TempDir,
    organization_id: OrganizationId,
    request: SourceCheckoutRequest,
    source: CheckedOutSource,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("source");
        std::fs::create_dir(&directory)?;
        let content = b"FROM scratch\n";
        std::fs::write(directory.join("Dockerfile"), content)?;
        let repository =
            GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?;
        let request = SourceCheckoutRequest::new(
            Uuid::now_v7(),
            repository.clone(),
            crate::modules::shared_kernel::domain::GitCommitSha::parse("a".repeat(40))?,
        )?;
        let source = CheckedOutSource {
            checkout_id: request.checkout_id,
            repository,
            commit_sha: request.commit_sha.clone(),
            directory,
            git_tree_id: "1".repeat(40),
            content_digest: format!("sha256:{}", "2".repeat(64)),
            file_count: 1,
            content_bytes: content.len() as u64,
            entries: vec![CheckedOutSourceEntry::new(
                "Dockerfile",
                CheckedOutSourceEntryKind::Regular,
                content.len() as u64,
                Sha256Digest::from_bytes(content),
            )?],
        };
        Ok(Self {
            _root: root,
            organization_id: OrganizationId::new(),
            request,
            source,
        })
    }
}

struct RecordingCheckout {
    source: CheckedOutSource,
    private: bool,
    calls: AtomicUsize,
    replays: AtomicUsize,
    credential_calls: AtomicUsize,
    removals: AtomicUsize,
}

impl RecordingCheckout {
    fn new(source: CheckedOutSource, private: bool) -> Self {
        Self {
            source,
            private,
            calls: AtomicUsize::new(0),
            replays: AtomicUsize::new(0),
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

    fn replays(&self) -> usize {
        self.replays.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ISourceCheckout for RecordingCheckout {
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
        if request
            != &(SourceCheckoutRequest {
                checkout_id: self.source.checkout_id,
                repository: self.source.repository.clone(),
                commit_sha: self.source.commit_sha.clone(),
            })
        {
            return Err(SourceCheckoutError::Conflict);
        }
        Ok(self.source.clone())
    }

    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        self.replays.fetch_add(1, Ordering::SeqCst);
        if request
            != &(SourceCheckoutRequest {
                checkout_id: self.source.checkout_id,
                repository: self.source.repository.clone(),
                commit_sha: self.source.commit_sha.clone(),
            })
        {
            return Err(SourceCheckoutError::Conflict);
        }
        Ok(self.source.clone())
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
) -> Result<(), Box<dyn std::error::Error>> {
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
    repository
        .complete(CompleteGithubConnection {
            flow_id,
            connection,
            event,
            completed_at: connected_at,
        })
        .await?;
    Ok(())
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
