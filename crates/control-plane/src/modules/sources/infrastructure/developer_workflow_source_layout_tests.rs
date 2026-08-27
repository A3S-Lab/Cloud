use super::*;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, GitCommitSha, OrganizationId, ProjectId, SourceRevisionId,
};
use crate::modules::sources::domain::{ExternalSourceRevision, NewExternalSourceRevision};
use crate::modules::sources::published::{BuildRecipe, GitProvider, GitRepository};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[tokio::test]
async fn exact_source_revision_is_checked_out_replayed_and_cleaned_before_publication(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input.clone())));
    let checkout = Arc::new(RecordingCheckout::new(
        fixture.directory.clone(),
        fixture.asset_acl.clone(),
        false,
    ));
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs.clone(), checkout.clone());

    let layout = adapter
        .acquire(fixture.request)
        .await?
        .expect("exact source layout");

    assert_eq!(inputs.requests(), vec![fixture.request]);
    assert_eq!(checkout.calls(), 2);
    assert_eq!(checkout.removals(), 1);
    assert_eq!(
        layout.identity().commit_sha,
        fixture.input.commit_sha().clone()
    );
    assert_eq!(
        layout.identity().source_identity_digest.as_str(),
        fixture
            .input
            .repository()
            .source_identity_digest(fixture.input.commit_sha())
    );
    assert_eq!(layout.entries().len(), 3);
    assert_eq!(
        layout
            .entry(ASSET_ACL_EVIDENCE_PATH)
            .and_then(SourceLayoutEntry::inspected_content),
        Some(fixture.asset_acl.as_slice())
    );
    assert!(layout
        .entry("Dockerfile")
        .is_some_and(|entry| entry.inspected_content().is_none()));
    assert!(layout
        .entry("docs-link")
        .is_some_and(|entry| entry.kind() == SourceLayoutEntryKind::Symlink));
    Ok(())
}

#[tokio::test]
async fn missing_exact_source_revision_never_acquires_provider_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(None));
    let checkout = Arc::new(RecordingCheckout::new(
        fixture.directory,
        fixture.asset_acl,
        false,
    ));
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert_eq!(adapter.acquire(fixture.request).await?, None);
    assert_eq!(checkout.calls(), 0);
    assert_eq!(checkout.removals(), 0);
    Ok(())
}

#[tokio::test]
async fn checkout_drift_fails_closed_and_still_cleans_owned_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input)));
    let checkout = Arc::new(RecordingCheckout::new(
        fixture.directory,
        fixture.asset_acl,
        true,
    ));
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert!(matches!(
        adapter.acquire(fixture.request).await,
        Err(BuildPlanSourceLayoutError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 2);
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn inspected_evidence_must_match_the_owner_checkout_entry(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input)));
    let checkout = Arc::new(RecordingCheckout::new(
        fixture.directory,
        b"different bytes\n".to_vec(),
        false,
    ));
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert!(matches!(
        adapter.acquire(fixture.request).await,
        Err(BuildPlanSourceLayoutError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 1);
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn missing_evidence_after_checkout_is_integrity_drift_and_is_cleaned(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    std::fs::remove_file(fixture.directory.join(ASSET_ACL_EVIDENCE_PATH))?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input)));
    let checkout = Arc::new(RecordingCheckout::new(
        fixture.directory,
        fixture.asset_acl,
        false,
    ));
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert!(matches!(
        adapter.acquire(fixture.request).await,
        Err(BuildPlanSourceLayoutError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 1);
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn cleanup_failure_prevents_layout_publication() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input)));
    let checkout = Arc::new(
        RecordingCheckout::new(fixture.directory, fixture.asset_acl, false).with_removal_failure(),
    );
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert!(matches!(
        adapter.acquire(fixture.request).await,
        Err(BuildPlanSourceLayoutError::Storage(_))
    ));
    assert_eq!(checkout.calls(), 2);
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[tokio::test]
async fn cleanup_failure_does_not_mask_an_integrity_failure(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let inputs = Arc::new(RecordingInputs::new(Some(fixture.input)));
    let checkout = Arc::new(
        RecordingCheckout::new(fixture.directory, b"different bytes\n".to_vec(), false)
            .with_removal_failure(),
    );
    let adapter = DeveloperWorkflowSourceLayoutAdapter::new(inputs, checkout.clone());

    assert!(matches!(
        adapter.acquire(fixture.request).await,
        Err(BuildPlanSourceLayoutError::Integrity(_))
    ));
    assert_eq!(checkout.calls(), 1);
    assert_eq!(checkout.removals(), 1);
    Ok(())
}

#[test]
fn source_layout_adapter_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DeveloperWorkflowSourceLayoutAdapter>();
}

struct Fixture {
    _root: tempfile::TempDir,
    directory: PathBuf,
    asset_acl: Vec<u8>,
    input: crate::modules::sources::published::SourceBuildInputSnapshot,
    request: BuildPlanSourceLayoutRequest,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("source");
        std::fs::create_dir_all(directory.join(".a3s"))?;
        let asset_acl = concat!(
            "asset {\n",
            "  kind = \"agent\"\n",
            "  schema = \"a3s.cloud.asset.v1\"\n",
            "  build {\n",
            "    context = \".\"\n",
            "    file = \"Dockerfile\"\n",
            "    platforms = [\"linux/amd64\"]\n",
            "  }\n",
            "}\n",
        )
        .as_bytes()
        .to_vec();
        std::fs::write(directory.join(".a3s").join("asset.acl"), &asset_acl)?;
        std::fs::write(directory.join("Dockerfile"), b"FROM scratch\n")?;

        let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            id: SourceRevisionId::new(),
            repository: GitRepository::parse(
                GitProvider::Github,
                "https://github.com/A3S-Lab/Cloud.git",
            )?,
            commit_sha: GitCommitSha::parse("a".repeat(40))?,
            recipe: BuildRecipe::dockerfile(
                BuildRecipe::SCHEMA,
                BuildRecipe::DOCKERFILE_KIND,
                ".",
                "Dockerfile",
                None,
                vec!["linux/amd64".into()],
            )?,
            accepted_at: chrono::Utc::now(),
        })?;
        let request = BuildPlanSourceLayoutRequest {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            source_revision_id: revision.id,
        };
        let input = crate::modules::sources::publish_source_build_input(&revision)?;
        Ok(Self {
            _root: root,
            directory,
            asset_acl,
            input,
            request,
        })
    }
}

struct RecordingInputs {
    input: Option<crate::modules::sources::published::SourceBuildInputSnapshot>,
    requests: Mutex<Vec<BuildPlanSourceLayoutRequest>>,
}

impl RecordingInputs {
    fn new(input: Option<crate::modules::sources::published::SourceBuildInputSnapshot>) -> Self {
        Self {
            input,
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<BuildPlanSourceLayoutRequest> {
        self.requests.lock().expect("input requests").clone()
    }
}

#[async_trait]
impl ISourceBuildInputQueryPort for RecordingInputs {
    async fn find_source_build_input(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        source_revision_id: SourceRevisionId,
    ) -> Result<
        Option<crate::modules::sources::published::SourceBuildInputSnapshot>,
        SourceBuildInputQueryError,
    > {
        self.requests
            .lock()
            .expect("input requests")
            .push(BuildPlanSourceLayoutRequest {
                organization_id,
                project_id,
                environment_id,
                source_revision_id,
            });
        Ok(self.input.clone())
    }
}

struct RecordingCheckout {
    directory: PathBuf,
    asset_acl: Vec<u8>,
    drift_on_replay: bool,
    fail_removal: bool,
    calls: AtomicUsize,
    removals: AtomicUsize,
    checkout_ids: Mutex<Vec<Uuid>>,
}

impl RecordingCheckout {
    fn new(directory: PathBuf, asset_acl: Vec<u8>, drift_on_replay: bool) -> Self {
        Self {
            directory,
            asset_acl,
            drift_on_replay,
            fail_removal: false,
            calls: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
            checkout_ids: Mutex::new(Vec::new()),
        }
    }

    fn with_removal_failure(mut self) -> Self {
        self.fail_removal = true;
        self
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn removals(&self) -> usize {
        self.removals.load(Ordering::SeqCst)
    }

    fn source(&self, request: &SourceCheckoutRequest, call: usize) -> CheckedOutSource {
        let dockerfile = b"FROM scratch\n";
        let link_target = b"docs";
        let mut content_digest = format!("sha256:{}", "3".repeat(64));
        if self.drift_on_replay && call == 1 {
            content_digest = format!("sha256:{}", "4".repeat(64));
        }
        let entries = vec![
            CheckedOutSourceEntry::new(
                ASSET_ACL_EVIDENCE_PATH,
                CheckedOutSourceEntryKind::Regular,
                self.asset_acl.len() as u64,
                Sha256Digest::from_bytes(&self.asset_acl),
            )
            .expect("asset entry"),
            CheckedOutSourceEntry::new(
                "Dockerfile",
                CheckedOutSourceEntryKind::Regular,
                dockerfile.len() as u64,
                Sha256Digest::from_bytes(dockerfile),
            )
            .expect("Dockerfile entry"),
            CheckedOutSourceEntry::new(
                "docs-link",
                CheckedOutSourceEntryKind::Symlink,
                link_target.len() as u64,
                Sha256Digest::from_bytes(link_target),
            )
            .expect("symlink entry"),
        ];
        CheckedOutSource {
            checkout_id: request.checkout_id,
            repository: request.repository.clone(),
            commit_sha: request.commit_sha.clone(),
            directory: self.directory.clone(),
            git_tree_id: "2".repeat(40),
            content_digest,
            file_count: entries.len(),
            content_bytes: entries.iter().map(CheckedOutSourceEntry::size_bytes).sum(),
            entries,
        }
    }
}

#[async_trait]
impl IAuthorizedSourceCheckout for RecordingCheckout {
    async fn checkout(
        &self,
        organization_id: OrganizationId,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        if organization_id.as_uuid().is_nil() {
            return Err(SourceCheckoutError::Invalid(
                "organization cannot be nil".into(),
            ));
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.checkout_ids
            .lock()
            .expect("checkout IDs")
            .push(request.checkout_id);
        Ok(self.source(request, call))
    }

    async fn replay(
        &self,
        request: &SourceCheckoutRequest,
    ) -> Result<CheckedOutSource, SourceCheckoutError> {
        if !self
            .checkout_ids
            .lock()
            .expect("checkout IDs")
            .contains(&request.checkout_id)
        {
            return Err(SourceCheckoutError::Integrity(
                "fixture checkout is unavailable for strict replay".into(),
            ));
        }
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.source(request, call))
    }

    async fn remove(&self, checkout_id: Uuid) -> Result<(), SourceCheckoutError> {
        if !self
            .checkout_ids
            .lock()
            .expect("checkout IDs")
            .contains(&checkout_id)
        {
            return Err(SourceCheckoutError::Conflict);
        }
        self.removals.fetch_add(1, Ordering::SeqCst);
        if self.fail_removal {
            return Err(SourceCheckoutError::Storage(
                "fixture cleanup failed".into(),
            ));
        }
        Ok(())
    }
}
