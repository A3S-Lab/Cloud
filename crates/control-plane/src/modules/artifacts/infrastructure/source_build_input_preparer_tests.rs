use super::*;
use crate::modules::artifacts::infrastructure::LocalNodeArtifactStore;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{
    GitCommitSha, GitProvider, GitRepository, NewExternalSourceRevision, SourceProviderCredential,
};
use crate::modules::sources::{GithubInstallationTokenIssuer, InMemoryGithubConnectionRepository};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn prepared_source_is_deterministic_and_replayed_without_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source_directory = root.path().join("source");
    tokio::fs::create_dir(&source_directory).await?;
    tokio::fs::write(source_directory.join("Dockerfile"), "FROM scratch\n").await?;
    tokio::fs::write(source_directory.join("message.txt"), "deterministic\n").await?;
    let (build, revision) = build_and_revision()?;
    let checkout = Arc::new(ReplayCheckout::new(
        checked_out_source(&revision, build.id.as_uuid(), source_directory),
        false,
    ));
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("artifacts"),
        16 * 1024 * 1024,
    )?);
    let preparer = preparer(root.path(), checkout.clone(), store)?;

    let first = preparer.prepare(&build, &revision).await?;
    let replay = preparer.prepare(&build, &revision).await?;
    assert_eq!(first, replay);
    assert_eq!(checkout.calls(), 4);
    assert_eq!(checkout.credential_calls(), 0);
    preparer.remove(&build).await?;
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn package_time_checkout_change_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let source_directory = root.path().join("source");
    tokio::fs::create_dir(&source_directory).await?;
    tokio::fs::write(source_directory.join("Dockerfile"), "FROM scratch\n").await?;
    let (build, revision) = build_and_revision()?;
    let checkout = Arc::new(ReplayCheckout::new(
        checked_out_source(&revision, build.id.as_uuid(), source_directory),
        true,
    ));
    let store = Arc::new(LocalNodeArtifactStore::new(
        root.path().join("artifacts"),
        16 * 1024 * 1024,
    )?);
    let preparer = preparer(root.path(), checkout.clone(), store)?;

    assert!(matches!(
        preparer.prepare(&build, &revision).await,
        Err(BuildInputPreparationError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 2);
    assert_eq!(checkout.credential_calls(), 0);
    Ok(())
}

fn preparer(
    root: &Path,
    checkout: Arc<ReplayCheckout>,
    artifacts: Arc<LocalNodeArtifactStore>,
) -> Result<SourceBuildInputPreparer, String> {
    SourceBuildInputPreparer::new(
        checkout,
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        artifacts,
        root.join("staging"),
        1_024,
        16 * 1024 * 1024,
    )
}

fn build_and_revision() -> Result<(BuildRun, ExternalSourceRevision), String> {
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let repository = GitRepository::parse(GitProvider::Github, "https://github.com/A3S-Lab/Cloud")?;
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: source_revision_id,
        repository,
        commit_sha: GitCommitSha::parse("a".repeat(40))?,
        recipe: crate::modules::sources::domain::BuildRecipe::dockerfile(
            crate::modules::sources::domain::BuildRecipe::SCHEMA,
            crate::modules::sources::domain::BuildRecipe::DOCKERFILE_KIND,
            ".",
            "Dockerfile",
            None,
            vec!["linux/amd64".into()],
        )?,
        accepted_at: chrono::Utc::now(),
    })?;
    Ok((
        BuildRun::reserve(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
            revision.accepted_at,
        ),
        revision,
    ))
}

fn checked_out_source(
    revision: &ExternalSourceRevision,
    checkout_id: Uuid,
    directory: PathBuf,
) -> CheckedOutSource {
    CheckedOutSource {
        checkout_id,
        repository: revision.repository.clone(),
        commit_sha: revision.commit_sha.clone(),
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
    calls: AtomicUsize,
    credential_calls: AtomicUsize,
    removals: AtomicUsize,
}

impl ReplayCheckout {
    fn new(source: CheckedOutSource, change_on_replay: bool) -> Self {
        Self {
            source,
            change_on_replay,
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
