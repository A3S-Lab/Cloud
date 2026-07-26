use a3s_cloud_control_plane::modules::shared_kernel::domain::{OrganizationId, SourceConnectionId};
use a3s_cloud_control_plane::modules::sources::domain::{
    GitProvider, GitReference, GitRepository, GithubInstallationId, GithubInstallationTokenRequest,
    IGithubInstallationTokenService, ISourceCheckout, ISourceResolver, SourceCheckoutRequest,
    SourceResolutionRequest,
};
use a3s_cloud_control_plane::modules::sources::{
    GitSourceCheckout, GithubInstallationTokenIssuer, GithubSourceResolver,
};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

const EVIDENCE_SCHEMA: &str = "a3s.cloud.g0-private-github-provider-evidence.v1";
const EVIDENCE_DIRECTORY_ENV: &str = "A3S_CLOUD_TEST_G0_EVIDENCE_DIR";
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
    let directory = tempfile::tempdir()?;
    let checkout = GitSourceCheckout::new(
        directory.path(),
        Duration::from_secs(120),
        100_000,
        512 * 1024 * 1024,
    )
    .map_err(test_error)?;
    let request =
        SourceCheckoutRequest::new(Uuid::now_v7(), repository, resolved.commit_sha.clone())
            .map_err(test_error)?;

    let accepted = checkout.checkout(&request, Some(&credential)).await?;
    assert_eq!(accepted.commit_sha, resolved.commit_sha);
    assert!(!accepted.directory.join(".git").exists());
    drop(credential);
    assert_eq!(checkout.checkout(&request, None).await?, accepted);
    checkout.remove(request.checkout_id).await?;
    assert!(!accepted.directory.exists());

    let evidence = PrivateGithubProviderEvidence {
        schema: EVIDENCE_SCHEMA,
        cloud_revision,
        provider: "github",
        repository_identity_digest,
        accepted_commit_digest: sha256(resolved.commit_sha.as_str().as_bytes()),
        git_tree_digest: sha256(accepted.git_tree_id.as_bytes()),
        content_digest: accepted.content_digest,
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
