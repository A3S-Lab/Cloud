use a3s_cloud_control_plane::modules::artifacts::{
    BuildArtifact, BuildRun, BuildSource, IBuildInputPreparer, INodeArtifactStore,
    NodeArtifactObjectStore, SourceBuildInputPreparer,
};
use a3s_cloud_control_plane::modules::shared_kernel::domain::{
    BuildRunId, EnvironmentId, IdempotencyRequest, IdempotentWrite, OrganizationId, ProjectId,
    RepositoryError, SourceConnectionId, SourceRevisionId,
};
use a3s_cloud_control_plane::modules::sources::domain::{
    AcceptSourceRevision, ExternalSourceRevision, GitProvider, GitReference, GitRepository,
    GithubInstallationId, GithubInstallationTokenRequest, IGithubInstallationTokenService,
    ISourceCheckout, ISourceResolver, ISourceRevisionRepository, NewExternalSourceRevision,
    SourceCheckoutRequest, SourceResolutionRequest,
};
use a3s_cloud_control_plane::modules::sources::published::BuildRecipe;
use a3s_cloud_control_plane::modules::sources::{
    GitSourceCheckout, GithubInstallationTokenIssuer, GithubSourceResolver,
    ISourceBuildInputQueryPort, InMemoryGithubConnectionRepository, SourceBuildInputQueryService,
};
use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use zeroize::Zeroizing;

const EVIDENCE_SCHEMA: &str = "a3s.cloud.g0-private-github-provider-evidence.v1";
const HANDOFF_SCHEMA: &str = "a3s.cloud.g0-private-source-handoff.v1";
const EVIDENCE_DIRECTORY_ENV: &str = "A3S_CLOUD_TEST_G0_EVIDENCE_DIR";
const HANDOFF_DIRECTORY_ENV: &str = "A3S_CLOUD_TEST_G0_PRIVATE_HANDOFF_DIR";
const CLOUD_REVISION_ENV: &str = "A3S_CLOUD_TEST_CLOUD_REVISION";

#[tokio::test]
#[ignore = "requires a real GitHub App installation and private repository"]
async fn real_github_installation_token_resolves_and_checks_out_a_private_repository(
) -> Result<(), Box<dyn std::error::Error>> {
    const PRIVATE_KEY_ENV: &str = "A3S_CLOUD_TEST_GITHUB_APP_PRIVATE_KEY";

    let cloud_revision = required(CLOUD_REVISION_ENV)?;
    validate_cloud_revision(&cloud_revision)?;
    let client_id = required("A3S_CLOUD_TEST_GITHUB_APP_CLIENT_ID")?;
    let installation_id = required("A3S_CLOUD_TEST_GITHUB_INSTALLATION_ID")?.parse::<u64>()?;
    let repository_url = required("A3S_CLOUD_TEST_GITHUB_PRIVATE_REPOSITORY")?;
    let branch = required("A3S_CLOUD_TEST_GITHUB_PRIVATE_BRANCH")?;
    let private_key = Zeroizing::new(required(PRIVATE_KEY_ENV)?);
    let repository =
        GitRepository::parse(GitProvider::Github, &repository_url).map_err(test_error)?;
    let repository_identity_digest = sha256(repository.identity().as_bytes());
    let issuer =
        GithubInstallationTokenIssuer::new(Duration::from_secs(30), client_id, PRIVATE_KEY_ENV)
            .map_err(test_error)?;
    let credential = issuer
        .issue(GithubInstallationTokenRequest {
            organization_id: OrganizationId::new(),
            connection_id: SourceConnectionId::new(),
            installation_id: GithubInstallationId::parse(installation_id).map_err(test_error)?,
            repository: repository.clone(),
            requested_at: Utc::now(),
        })
        .await?;
    let credential_issued_at = credential.issued_at();
    let credential_expires_at = credential.expires_at();
    let credential_debug = format!("{credential:?}");
    if !credential_debug.contains("<redacted>")
        || credential_debug.contains(private_key.as_str())
        || credential_debug.contains(['\r', '\n'])
    {
        return Err(test_error(
            "private GitHub credential Debug output is not safely redacted".into(),
        ));
    }
    let resolver = GithubSourceResolver::new(Duration::from_secs(30)).map_err(test_error)?;
    let resolved = resolver
        .resolve(
            &SourceResolutionRequest {
                repository: repository.clone(),
                reference: GitReference::parse("branch", &branch).map_err(test_error)?,
            },
            Some(&credential),
        )
        .await?;
    let organization_id = OrganizationId::new();
    let project_id = ProjectId::new();
    let environment_id = EnvironmentId::new();
    let source_revision_id = SourceRevisionId::new();
    let accepted_at = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .ok_or_else(|| {
            test_error("private source timestamp exceeds PostgreSQL precision".into())
        })?;
    let recipe = BuildRecipe::dockerfile(
        BuildRecipe::SCHEMA,
        BuildRecipe::DOCKERFILE_KIND,
        ".",
        "Containerfile",
        None,
        vec!["linux/amd64".into()],
    )?;
    let revision = ExternalSourceRevision::accept(NewExternalSourceRevision {
        organization_id,
        project_id,
        environment_id,
        id: source_revision_id,
        repository: repository.clone(),
        commit_sha: resolved.commit_sha.clone(),
        recipe,
        accepted_at,
    })?;
    let build = BuildRun::reserve(
        organization_id,
        project_id,
        environment_id,
        source_revision_id,
        revision.accepted_at,
    );
    let source_inputs = SourceBuildInputQueryService::new(Arc::new(SingleRevisionRepository {
        revision: revision.clone(),
    }));
    let input = source_inputs
        .find_source_build_input(
            organization_id,
            project_id,
            environment_id,
            source_revision_id,
        )
        .await?
        .ok_or_else(|| test_error("private Source build input was not found".into()))?;
    let source = BuildSource::from_source_input(&input)?;
    let directory = tempfile::tempdir()?;
    let checkout = Arc::new(
        GitSourceCheckout::new(
            directory.path(),
            Duration::from_secs(120),
            100_000,
            512 * 1024 * 1024,
        )
        .map_err(test_error)?,
    );
    let request =
        SourceCheckoutRequest::new(build.id.as_uuid(), repository, resolved.commit_sha.clone())
            .map_err(test_error)?;

    let accepted = checkout.checkout(&request, Some(&credential)).await?;
    assert_eq!(accepted.commit_sha, resolved.commit_sha);
    assert!(!accepted.directory.join(".git").exists());
    drop(credential);
    assert_eq!(checkout.checkout(&request, None).await?, accepted);
    let handoff_directory = secure_handoff_directory(&required(HANDOFF_DIRECTORY_ENV)?).await?;
    let artifact_store = Arc::new(NodeArtifactObjectStore::local(
        handoff_directory.join("artifact-store"),
        512 * 1024 * 1024,
    )?);
    let preparer = SourceBuildInputPreparer::new(
        checkout.clone(),
        Arc::new(InMemoryGithubConnectionRepository::new()),
        Arc::new(GithubInstallationTokenIssuer::disabled()),
        artifact_store.clone(),
        handoff_directory.join("input-staging"),
        100_000,
        512 * 1024 * 1024,
    )?;
    let prepared = preparer.prepare(&build, &source).await?;
    if prepared.source_content_digest != accepted.content_digest {
        return Err(test_error(
            "production build input changed the accepted private checkout digest".into(),
        ));
    }
    export_artifact(
        artifact_store.as_ref(),
        &prepared.artifact,
        &handoff_directory.join("source-input.tar"),
    )
    .await?;
    write_private_handoff(
        &handoff_directory.join("source-handoff.json"),
        &PrivateSourceHandoff {
            schema: HANDOFF_SCHEMA,
            cloud_revision: cloud_revision.clone(),
            build_run_id: build.id,
            revision,
            source_content_digest: prepared.source_content_digest.clone(),
            input_artifact: prepared.artifact.clone(),
        },
    )
    .await?;
    preparer.remove(&build).await?;
    assert!(!accepted.directory.exists());
    remove_private_working_state(&handoff_directory).await?;

    let evidence = PrivateGithubProviderEvidence {
        schema: EVIDENCE_SCHEMA,
        cloud_revision,
        provider: "github",
        repository_identity_digest,
        accepted_commit_digest: sha256(resolved.commit_sha.as_str().as_bytes()),
        git_tree_digest: sha256(accepted.git_tree_id.as_bytes()),
        content_digest: accepted.content_digest,
        build_input_digest: prepared.artifact.digest,
        build_input_bytes: prepared.artifact.size_bytes,
        build_run_identity_digest: sha256(build.id.to_string().as_bytes()),
        file_count: accepted.file_count,
        content_bytes: accepted.content_bytes,
        credential_issued_at,
        credential_expires_at,
        completed_at: Utc::now(),
        checks: PrivateGithubProviderChecks {
            credential_debug_redacted: true,
            credential_free_replay: true,
            git_metadata_absent: true,
            checkout_removed: true,
            production_build_input_prepared: true,
            private_handoff_isolated: true,
        },
    };
    let mut encoded = serde_json::to_vec_pretty(&evidence)?;
    if contains(&encoded, private_key.as_bytes())
        || contains(&encoded, repository_url.as_bytes())
        || contains(&encoded, branch.as_bytes())
    {
        return Err(test_error(
            "private GitHub provider evidence contains protected input".into(),
        ));
    }
    encoded.push(b'\n');
    write_evidence(&required(EVIDENCE_DIRECTORY_ENV)?, &encoded).await?;
    Ok(())
}

struct SingleRevisionRepository {
    revision: ExternalSourceRevision,
}

#[async_trait]
impl ISourceRevisionRepository for SingleRevisionRepository {
    async fn find(
        &self,
        _organization_id: OrganizationId,
        _source_revision_id: SourceRevisionId,
    ) -> Result<ExternalSourceRevision, RepositoryError> {
        Ok(self.revision.clone())
    }

    async fn replay_acceptance(
        &self,
        _idempotency: &IdempotencyRequest,
    ) -> Result<Option<ExternalSourceRevision>, RepositoryError> {
        Ok(None)
    }

    async fn accept(
        &self,
        _request: AcceptSourceRevision,
    ) -> Result<IdempotentWrite<ExternalSourceRevision>, RepositoryError> {
        Err(RepositoryError::Storage(
            "private Source fixture is immutable".into(),
        ))
    }

    async fn list(
        &self,
        _organization_id: OrganizationId,
        _project_id: ProjectId,
        _environment_id: EnvironmentId,
    ) -> Result<Vec<ExternalSourceRevision>, RepositoryError> {
        Ok(vec![self.revision.clone()])
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivateGithubProviderEvidence {
    schema: &'static str,
    cloud_revision: String,
    provider: &'static str,
    repository_identity_digest: String,
    accepted_commit_digest: String,
    git_tree_digest: String,
    content_digest: String,
    build_input_digest: String,
    build_input_bytes: u64,
    build_run_identity_digest: String,
    file_count: usize,
    content_bytes: u64,
    credential_issued_at: chrono::DateTime<Utc>,
    credential_expires_at: chrono::DateTime<Utc>,
    completed_at: chrono::DateTime<Utc>,
    checks: PrivateGithubProviderChecks,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PrivateGithubProviderChecks {
    credential_debug_redacted: bool,
    credential_free_replay: bool,
    git_metadata_absent: bool,
    checkout_removed: bool,
    production_build_input_prepared: bool,
    private_handoff_isolated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrivateSourceHandoff {
    schema: &'static str,
    cloud_revision: String,
    build_run_id: BuildRunId,
    revision: ExternalSourceRevision,
    source_content_digest: String,
    input_artifact: BuildArtifact,
}

async fn secure_handoff_directory(value: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let directory = PathBuf::from(value);
    if !directory.is_absolute() {
        return Err(test_error(
            "G0 private source handoff directory must be absolute".into(),
        ));
    }
    tokio::fs::create_dir_all(&directory).await?;
    let metadata = tokio::fs::symlink_metadata(&directory).await?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(test_error(
            "G0 private source handoff directory is not an owned directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(tokio::fs::canonicalize(directory).await?)
}

async fn export_artifact(
    store: &NodeArtifactObjectStore,
    artifact: &BuildArtifact,
    destination: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    artifact.validate()?;
    let reference = a3s_runtime::contract::ArtifactRef {
        uri: artifact.uri.clone(),
        digest: artifact.digest.clone(),
        media_type: artifact.media_type.clone(),
    };
    let mut opened = store.open(&reference).await?;
    if opened.descriptor.size_bytes != artifact.size_bytes {
        return Err(test_error(
            "prepared private build input changed size before handoff".into(),
        ));
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(destination).await?;
    let copied = tokio::io::copy(&mut opened.reader, &mut file).await?;
    if copied != artifact.size_bytes {
        return Err(test_error(
            "prepared private build input handoff was truncated".into(),
        ));
    }
    file.flush().await?;
    file.sync_all().await?;
    secure_file(destination).await
}

async fn write_private_handoff(
    destination: &Path,
    handoff: &PrivateSourceHandoff,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut encoded = serde_json::to_vec_pretty(handoff)?;
    encoded.push(b'\n');
    let temporary = destination.with_extension(format!("{}.tmp", Uuid::now_v7()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temporary).await?;
    file.write_all(&encoded).await?;
    file.flush().await?;
    file.sync_all().await?;
    secure_file(&temporary).await?;
    tokio::fs::rename(&temporary, destination).await?;
    Ok(())
}

async fn secure_file(_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(())
}

async fn remove_private_working_state(
    handoff_directory: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for path in ["artifact-store", "input-staging"] {
        match tokio::fs::remove_dir_all(handoff_directory.join(path)).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn write_evidence(
    directory: &str,
    evidence: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = Path::new(directory);
    if !directory.is_absolute() {
        return Err(test_error(
            "G0 provider evidence directory must be absolute".into(),
        ));
    }
    tokio::fs::create_dir_all(directory).await?;
    let temporary = directory.join(format!(".github-private-source-{}.tmp", Uuid::now_v7()));
    let destination = directory.join("github-private-source.json");
    tokio::fs::write(&temporary, evidence).await?;
    tokio::fs::rename(&temporary, destination).await?;
    Ok(())
}

fn validate_cloud_revision(revision: &str) -> Result<(), Box<dyn std::error::Error>> {
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(test_error(
            "G0 provider Cloud revision must be a full lowercase Git SHA".into(),
        ));
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    std::env::var(name).map_err(|_| test_error(format!("{name} is required")))
}

fn test_error(message: String) -> Box<dyn std::error::Error> {
    std::io::Error::other(message).into()
}
