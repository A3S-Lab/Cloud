use super::*;
use crate::modules::artifacts::application::{
    IExternalSourceArchivePort, OpenExternalSourceArchive,
};
use crate::modules::artifacts::infrastructure::NodeArtifactObjectStore;
use crate::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, OrganizationId, ProjectId, Sha256Digest, SourceRevisionId,
};
use crate::modules::sources::domain::{
    ExternalSourceRevision, GitCommitSha, GitProvider, GitRepository, NewExternalSourceRevision,
};
use crate::modules::sources::publish_source_build_input;
use crate::modules::sources::published::BuildRecipe;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

#[tokio::test]
async fn external_archive_port_is_the_only_source_provider_boundary(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let (build, revision) = build_and_revision()?;
    let input = publish_source_build_input(&revision)?;
    let source = BuildSource::from_source_input(&input)?;
    let archive_bytes = b"deterministic external Source tar bytes".to_vec();
    let external = Arc::new(RecordingExternalArchivePort::new(archive_bytes.clone()));
    let store = Arc::new(NodeArtifactObjectStore::local(
        root.path().join("artifacts"),
        16 * 1024 * 1024,
    )?);
    let preparer = SourceBuildInputPreparer::new(external.clone(), store);

    let first = preparer.prepare(&build, &source).await?;
    let replay = preparer.prepare(&build, &source).await?;
    assert_eq!(first, replay);
    assert_eq!(
        first.artifact.digest,
        format!("sha256:{:x}", Sha256::digest(&archive_bytes))
    );
    let requests = external.requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| {
        request.organization_id() == build.organization_id
            && request.build_run_id() == build.id
            && request.repository() == &revision.repository
            && request.commit_sha() == &revision.commit_sha
    }));
    drop(requests);
    preparer.remove(&build).await?;
    assert_eq!(external.removals.load(Ordering::SeqCst), 1);
    Ok(())
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

struct RecordingExternalArchivePort {
    archive_bytes: Vec<u8>,
    requests: Mutex<Vec<ExternalSourceArchiveRequest>>,
    removals: AtomicUsize,
}

impl RecordingExternalArchivePort {
    fn new(archive_bytes: Vec<u8>) -> Self {
        Self {
            archive_bytes,
            requests: Mutex::new(Vec::new()),
            removals: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl IExternalSourceArchivePort for RecordingExternalArchivePort {
    async fn prepare(
        &self,
        request: ExternalSourceArchiveRequest,
    ) -> Result<OpenExternalSourceArchive, BuildInputPreparationError> {
        request
            .validate()
            .map_err(BuildInputPreparationError::Invalid)?;
        self.requests.lock().await.push(request);
        OpenExternalSourceArchive::new(
            Sha256Digest::parse(format!("sha256:{}", "b".repeat(64)))
                .map_err(BuildInputPreparationError::Invalid)?,
            Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(&self.archive_bytes)))
                .map_err(BuildInputPreparationError::Invalid)?,
            self.archive_bytes.len() as u64,
            Box::pin(Cursor::new(self.archive_bytes.clone())),
        )
        .map_err(BuildInputPreparationError::Invalid)
    }

    async fn remove(&self, build_run_id: BuildRunId) -> Result<(), BuildInputPreparationError> {
        if build_run_id.as_uuid().is_nil() {
            return Err(BuildInputPreparationError::Invalid(
                "BuildRun ID is invalid".into(),
            ));
        }
        self.removals.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
